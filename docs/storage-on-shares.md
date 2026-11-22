# Storage on network shares and cloud-synced folders

Findings of rabalyn/shale#470 (part of the umbrella #467). Spec rows: `NET-*` in
[`spec/vault-and-files.md`](spec/vault-and-files.md). Test stacks: `tests/syncthing-env/` (ran),
`tests/smb-env/` (rootless through rclone FUSE: *ran*, a stand-in, not the kernel client; the kernel mount needs root: not run) and
`tests/nfs-env/` (kernel mount, root: not run; see "Limits of the evidence").

**Evidence is labelled.** *Ran* = a test in this repository observed it on a real system (Shale's own code against
a real Syncthing pair, or against the local file system). *Documented* = well-known behaviour of the system as its
documentation or the kernel's documentation describes it, **not** observed here. *Code* = read from Shale's source.

## Summary

| Topic | Verdict |
|---|---|
| Atomic save, temp files | Fine. `.shale-tmp-*` is hidden and never listed by the scan; a sync client sees it only if it scans during the few milliseconds of a save. Orphans from a crash are swept after a day (NET-016, #476). |
| Change detection | Fine on all systems: a full rescan every 3 s, no file watcher, stamps compare mtime, size, inode, ctime. **Defect**: the rescan runs on the UI thread and freezes on a slow share ([#473](https://github.com/rabalyn/shale/issues/473)). |
| Watching | No watcher exists, so nothing is missed on NFS/SMB; no polling to add (already 3 s). |
| Locks | Shale takes no lock in the vault. The tree lock is in-process; the instance lock is in the local data folder. Two Shales on two machines are protected only by DATA-003. |
| Cloud conflict files | Recognised by name (Syncthing, Dropbox, Nextcloud, ownCloud, Seafile, Shale's own), marked in the file tree and in the note, with compare and keep (NET-015, #475). OneDrive, iCloud and Google Drive copies cannot be told from a note by name. |
| `.shale/` from two devices | Fixed for a vault found synced (NET-016, #476): `state.json` and `undo/` live in the data folder, conflict copies are swept. (Before: `state.sync-conflict-…json` stayed for ever and devices overwrote each other's tabs.) |
| Case and Unicode names | Case: fine (DATA-013, VAULT-026..028). NFC and NFD spellings are one name for links (NET-014, [#474](https://github.com/rabalyn/shale/issues/474)); Syncthing even renames the local file. |
| A repository in a synced folder | Unsafe, confirmed by how git works and by the clients' behaviour; local-only history must live outside the vault (owner decision #467). |

## 1. Atomic save and temp files

**What Shale does** (*code*, `src/fsutil.rs`): `write_atomic` writes `<folder>/.shale-tmp-<pid>-<n>-<nanos>` with
`create_new`, `fsync`s it, copies the permissions, renames it over the target, then `fsync`s the folder
(DATA-004, DATA-013). `create_new`, `copy_new` and the case-only rename use the same names. `rename_noreplace`
uses `renameat2(RENAME_NOREPLACE)`, falling back to a hard link or a check first. The vault scan skips every name
that starts with `.` (`vault.rs::walk`), so temp files, `.shale/`, `.trash/` and `.git/` are never notes.

**Local and Syncthing** (*ran*, `net_001_…`, `syncthing_env_saves_arrive_…`): 40 saves in a row leave no temp
file; a leftover `.shale-tmp-*` is not listed by the scan. 30 saves of one note over 4.5 s on device A, with B's
folder polled every 10 ms: B never listed a `.shale-tmp-*` name (it only ever saw `N.md`, and once Syncthing's own
`.syncthing.N.md.tmp`, a dot file). The last version reached B 15 s after the first save (Syncthing batches).

**SMB** (*documented*): rename over an existing file is atomic on the server for SMB2/3 when the client uses
`FILE_RENAME_INFORMATION` with replace; the Linux CIFS client and Samba do this, so the replacing rename is atomic
there. Windows clients renaming over a file another client has open get a sharing violation (DATA-013 retries for
about a second). Strict-sync/`fsync` is passed to the server only with `cache=strict` (the default).
`RENAME_NOREPLACE` is not supported by CIFS (`EINVAL`): Shale's fallback (hard link, then check) applies; a hard link may
not work either, so the check-then-rename fallback can run, which has a small race window between two
machines. This is a documented limit of `rename_noreplace`, not a data-loss path: a new file is made with
`create_new`'s temp + the check, an existing one is never replaced by it.
**NFS** (*documented*): `rename(2)` is atomic on the server (NFSv3 and v4). `RENAME_NOREPLACE` is supported from
Linux 5.x over NFSv4 only when the server agrees; else the fallback. Close-to-open consistency means another client
sees the new file after it re-opens it; the `fsync` before the rename makes sure the data is on the server first.
Silly-rename: a file deleted or replaced while another process on the *same* client holds it open becomes
`.nfsXXXX` in the folder (*documented*). Shale keeps no note file open, so these appear only when another program does, and they are dot files, ignored by the scan.

**Cloud clients** (*documented* unless stated): a client that uploads while a file is being written can pick up a
half-written file. The save writes the temp file in the same folder and renames within milliseconds, so a client
that scans or reacts to file-system events (inotify, FSEvents, ReadDirectoryChangesW) can see
`.shale-tmp-…` created, written and renamed. No client knows the name `.shale-tmp-*`; whether a hidden file is synced
depends on the client's own ignore list (Syncthing: `.stignore`; Nextcloud: the sync exclude list; Dropbox: no
ignore list by name). A temp file that was picked up is
uploaded, then deleted a moment later: harmless but noisy, and with a large file (a pasted video) the upload may
take long. **No data is at risk**: the final name appears only through the rename, as one complete file.
iCloud Drive on macOS uploads the final file once complete (*documented*: it works on closed files, via a file
coordinator).
A `.shale-tmp-*` that a crash left is swept after a day (NET-016, #476).

## 2. Fingerprints and external change detection

**What Shale does** (*code*, `vault.rs`): the open note remembers a content `Stamp` (length + FNV-1a hash,
DATA-003) and the scan compares a `FileStamp` (mtime, size, inode, ctime) to decide whether to re-read a file
(DATA-002). Before a save the file is read and compared by content, so a clock or timestamp problem can never
cause a lost update or a false conflict; the save-time check does not look at times at all.

- **mtime precision** (*documented*): FAT/exFAT 2 s, SMB servers on ext4/NTFS 100 ns to 1 s, HFS+ 1 s, APFS 1 ns, NFS
  as the server's file system. A change within one tick keeps the same mtime; the scan also compares size, inode and
  ctime, and a same-length in-place write is caught by ctime where there is one. *Ran* (`net_001_…`): an in-place
  same-length write and a same-length atomic replace, immediately after the save, are both detected on the local
  file system (ext4/btrfs/tmpfs: ns timestamps). Where ctime is also coarse (FAT-backed shares) a same-length change in
  the same 2 s is a residual risk for the **scan** only; a save never trusts times (DATA-003).
- **Clock skew** (*documented*): timestamps are the server's clock for SMB and NFS (the client asks the server) and
  the client's own for cloud clients. Shale compares a file's times only with its own earlier values of the same
  file, never with the local clock, so skew is harmless. The only use of the local clock is the name of a conflict
  copy (`Name (conflict YYYY-MM-DD HHMM).ext`).
- **NFS attribute caching** (*documented*): `acregmin=3` `acregmax=60` `acdirmin=30` `acdirmax=60` seconds. A file
  changed by another machine can show old attributes for up to a minute, and a *new* file in a folder
  can be missing from a directory listing for up to `acdirmax` (default 60 s). So "within 3 s" (DATA-002) holds
  for local disks and cloud folders (the cloud client writes locally) and becomes "within about a minute" on NFS
  unless mounted with `actimeo=3` or `noac`. The save-time check reads the file's content (open revalidates
  with close-to-open consistency) and is not affected by the attribute cache.
- **Cloud clients** (*ran*, Syncthing): a note that arrives from the other device is found by the next scan of
  the open vault (the test saw it the moment the file was there), and a `.syncthing.*.tmp` of the client is not a note.
  Dropbox/OneDrive/iCloud placeholder files (*documented*): a file that is only online is a stub; reading it makes
  the client download it; the rescan reads it on a worker thread (NET-013, [#473](https://github.com/rabalyn/shale/issues/473)), only the first read at start is on the UI thread.

## 3. File watching

*Code*: Shale has **no file watcher** (no `notify`); `ShaleApp::run_frame` rescans every 3 s. inotify/FSEvents
would miss other machines' changes on NFS/SMB (*documented*: the kernel only reports changes made through this
client; an inotify watch on a cifs/nfs mount sees local writes only), so polling is the only correct choice, and
it is what exists. The scan runs on a worker thread and backs off to ten times its duration when it is slow
(NET-013, [#473](https://github.com/rabalyn/shale/issues/473)); `Vault::open` at start is still synchronous. A scan costs a directory walk and one `stat` per file:
local microseconds, a LAN share about a millisecond per file (*documented* round trips), so a vault of 5 000
notes needs seconds per scan on SMB; the window no longer waits for it.

## 4. Locks

*Code*: the **vault tree lock** (`gitsync::TreeLock`, SYNC-013) is an in-process mutex, not a file lock: it
orders the window against the sync thread of the same process and does not work or fail on any file system. The
**instance lock** (`<data folder>/shale/instance.lock`, DATA-012, `fsutil::try_lock_file`: `flock`, `LockFileEx`)
is in the local data folder; if the home folder is on NFS it is a lock on NFS: Linux implements `flock` on NFS as a
whole-file POSIX byte-range lock, which works across clients with a running lock daemon (NFSv3 needs `rpc.statd`/
lockd and says `ENOLCK` without) (*documented*); on failure Shale starts anyway and says why (DATA-012). The
**`env_lock`** is test-only, in `$XDG_RUNTIME_DIR` (local tmpfs). **SMB** (*documented*): the Linux CIFS client
translates `flock` to byte-range locks on the server; oplocks/leases do not matter to Shale (no open files are
kept). The tests observe it (`check_lock`, `smb_env_`/`nfs_env_` when run): the observation line says whether a
second `flock` by the same process, and by another process, is refused.
**Consequence**: nothing in Shale relies on a lock *in the vault*, so a vault on a share needs no working
locking. Two Shales on two machines on one vault are protected only by the content check at save time (DATA-003)
plus conflict copies, which is what we want. Documented limitation (NET-004).

## 5. Cloud clients' conflict files

*Ran* (Syncthing): two devices change a note at the same time; after the sync both devices hold `N.md` and
`N.sync-conflict-20261121-142440-7JJWLPP.md`, with both versions. The extension stays last, so Shale lists the
conflict file as a note and loses nothing. Shale recognises the name (NET-015, [#475](https://github.com/rabalyn/shale/issues/475)):
the copy gets a badge in the file tree and a banner with Compare, Keep this one and Keep the other, and is left out of
search, links, the graph, tasks and tags while its original exists. Patterns of the other clients are *documented*
(the last column says whether Shale recognises them):

| Client | Conflict file name | Recognised |
|---|---|---|
| Syncthing | `Name.sync-conflict-YYYYMMDD-HHMMSS-DEVICEID.ext` (ran) | yes |
| Dropbox | `Name (<User's> conflicted copy YYYY-MM-DD).ext`, a counter `Name (<User's> conflicted copy YYYY-MM-DD) (1).ext` | yes |
| Nextcloud / ownCloud | `Name (conflicted copy YYYY-MM-DD HHMMSS).ext` (Nextcloud); `Name_conflict-YYYYMMDD-HHMMSS.ext` (ownCloud) | yes |
| OneDrive | `Name-<COMPUTERNAME>.ext` (personal), `Name-<COMPUTERNAME>-<n>.ext`; SharePoint may keep both with a numeric suffix | no |
| iCloud Drive | `Name 2.ext` | no |
| Shale's own (SYNC-008, DATA-003) | `Name (conflict YYYY-MM-DD HHMM).ext` | yes |
| Google Drive for desktop | `Name (1).ext` | no |
| Seafile | `Name (SFConflict <user> YYYY-MM-DD-HH-MM-SS).ext` | yes |

OneDrive, iCloud and Google Drive names cannot be told from a note called `Plan 2` by the name alone, so the
detection (NET-015) is only done for the patterns that carry a keyword; the strict form of each is in the spec row.

## 6. `.shale/` state and undo files written by two devices

*Ran* (`syncthing_env_vault_state_…`): both devices save `.shale/state.json` (the open tabs) while the folder is
paused, then it resumes: one version wins at `state.json`, the other stays as `state.sync-conflict-….json`;
Shale loads the winner without a problem and the file is not a note. Nothing removes the conflict file
(the cleanup of USE-023 prunes undo files only) and the devices overwrite each other's tabs and panes. The undo
histories (EDIT-022: `.shale/undo/<hash of the note path>.json`, one file per note, the same name on both
devices) collide the same way (*code*; same mechanism). Git sync leaves `undo/` out (SYNC-006), a cloud client does not.
**Consequence** (fixed, NET-016, #476): device-local state is kept outside the synced folder for such vaults, and
conflict files of it are swept.

## 7. Case-insensitive and normalising file systems; names illegal elsewhere

- **Case** (*code*, *ran*): `create_new` of `plan.md` next to `Plan.md` either makes two files (Linux) or
  fails with `AlreadyExists` and leaves the first intact (macOS, Windows, SMB with `nocase`); the check passes both
  ways (`check_names`). A case-only rename works through a temp name (DATA-013, tested). Syncthing carried a
  case-only rename to the other device correctly (*ran*, `syncthing_env_a_case_only_…`). Two files that differ
  only in case from a Linux device arrive at a Windows/macOS device as one and a conflict in the client
  (*documented*); VAULT-028 ("Check names for Windows and macOS…") finds them.
- **Unicode** (*ran*): a note made as NFD (`e` + U+0301) on device A arrived on B as one note with the NFC
  name, and Syncthing's log says it *renamed the file on A to NFC* ("Normalized UTF8 encoding of file name").
  macOS/APFS (*documented*) keeps the bytes it was given but compares normalisation-insensitively; HFS+ and SMB
  servers on macOS store NFD. Shale compares names in NFC (then lower case), so `[[Café]]` (NFC, typed on Linux)
  finds a note whose name is NFD; the file name on disk is never changed (NET-014,
  [#474](https://github.com/rabalyn/shale/issues/474)). Two files that differ only in normal form (Linux) stay two
  notes that share a name (the usual ambiguity rules).
- **Illegal names** (*code*): everything Shale writes is made portable (VAULT-026, VAULT-027), a name from outside
  is checked on demand (VAULT-028). A path over 260 characters on Windows is handled by `\\?\` (DATA-013).

## 8. A repository inside such a folder

A git repository is thousands of small files that change together: `index`, `HEAD`, refs, pack files and the
object store. A sync client moves them one by one, in any order, at any time, and renames or re-creates them as
conflicts. Consequences (*documented* behaviour of git and of the clients, and the reason for the decision in
#467):

- A client uploads `.git/index` or a ref before the objects it names arrive on the other device: a half repository,
  `fatal: bad object`, `index file corrupt`.
- Two devices commit independently: the clients make `HEAD (conflicted copy)`, `index 2`, `packed-refs-<host>`;
  git does not read them, and the repository silently forks or loses a ref.
- Lock files (`index.lock`, `refs/…lock`) are synced to the other device and block it ("another git process
  seems to be running"), or a stale one is created by a restored copy.
- Packs and loose objects are written read-only and renamed; Dropbox/OneDrive may hold them open while uploading
  (sharing violation on Windows), and a delete-and-recreate by `git gc` shows up as conflicts.
- `core.fsmonitor`, file modes and `core.ignorecase` differ between the machines sharing the folder.

I did **not** corrupt a repository on purpose in a test: the Syncthing pair could do it
(`git init` in the shared folder, commit on both sides while paused, resume) but the outcome would be a
demonstration of a documented failure, not new information, and git's own documentation, Dropbox's and
Nextcloud's support pages all warn against it. The verdict stands: **local-only history keeps its repository
outside the vault** (`<data folder>/shale/history/<vault-id>`, separate git-dir), and only sync with a server keeps
`.git` inside the vault, on a folder no cloud client syncs. The detection of #469 (`gitsync/detect.rs`, SYNC-059…SYNC-063)
warns when a vault with `.git` is in such a folder, and moves the repository out when the vault has no server.

## SMB through rclone FUSE (rootless stand-in, not the kernel client)

[#477](https://github.com/rabalyn/shale/issues/477), spec NET-018. `tests/smb-env/smb-env.sh fuse-mount` mounts the Samba share of the test
server (smbd 4.12, SMB 3, user `shale`) with `rclone mount` (rclone v1.70.3, taken from the pinned image `docker.io/rclone/rclone:1.70.3`,
or a system rclone) on a folder of the user: no root, only `/dev/fuse` and `fusermount3`. The suite is `smb_fuse_env`
(`.forgejo/scripts/env-tests.sh run`, run by `scripts/land.sh` for a change of the storage code; the Forgejo job container has no
`/dev/fuse`, so the tag CI skips it with a note). Everything below is *ran* on this client, and **only this client**: it is rclone's SMB client
and its VFS layer behind FUSE, not the Linux kernel's CIFS client. It says what Shale's own code does on a share served by a real
smbd; it says nothing about CIFS-specific behaviour (oplocks and leases, `RENAME_NOREPLACE` returning `EINVAL`, the kernel's
attribute cache, byte-range locks sent to the server, the real mount options). Those stay *documented* until the manual run below.

Mount options (the reasons are in the header of `smb-env.sh`): `--vfs-cache-mode writes` (a file opened for writing is buffered locally and
uploaded at close, which makes writes at an offset and Shale's temp file then rename work; `off` cannot write at an offset, `full` would cache
reads and hide another client's change), `--vfs-write-back 0s`, `--dir-cache-time 1s`, `--attr-timeout 1s`, `--vfs-cache-max-age 1m`.

**Observed** (`cargo test --bin shale -- --ignored smb_fuse_env --nocapture`; all five checks pass):

| Check | Observation |
|---|---|
| Atomic save (NET-001) | 40 saves leave no temp file; `.shale-tmp-*` is not listed. Passes. |
| Outside change (NET-002, NET-003) | An in-place change of the same length and an atomic replace of the same length are both found by the next scan, and a save over a change by "another device" is a conflict copy. Modification time is finer than a second (millisecond parts seen); file time minus the local clock: about -0.001 s (same host). |
| Lock (NET-004) | A second `flock` by the same process is refused, and so is one from another process. This is the kernel's local lock emulation for a FUSE file system that implements no `flock`: rclone takes **no lock on the server**, so another client of the share would not be excluded. The test expects exclusion here for that reason (`storage_checks(…, true)`); the kernel client's check only reports (`false`). |
| Names (NET-005) | Letter case is **kept** (`Plan.md` and `plan.md` are two files), and NFC and NFD spellings of `Café.md` are the same file (the second create fails with `AlreadyExists`, one note in the vault). A case-only rename works. |
| Detection (SYNC-059) | `detect_real` reports the mount as a network folder, file system `fuse.rclone`. |

**Differences from the kernel CIFS client that matter for Shale:**

- **Write-back.** rclone uploads a closed file after `--vfs-write-back` (default 5 s). With the default, an in-place change of the same length keeps
  the old modification time until the upload, so the next scan in the same tick missed it (`an in-place change of the same length is detected` failed
  on the first run). `--vfs-write-back 0s` removes that; the kernel client writes through at close. A vault on rclone's own mount with the
  default would see a change after the upload and the next 3 s scan: late, never lost (NET-003 compares content).
- **Locks.** No byte-range or `flock` lock reaches the server (above); the kernel client sends them. So `check_lock` expects exclusion here and only reports there.
- **Letter case.** Kept here. The kernel client may differ (the server's `case sensitive` setting and the client's `nocase` option decide); the manual run observes it. Shale handles both (NET-005, DATA-013).
- **Unicode.** NFC and NFD names are one file here (rclone's SMB backend or the server folds them); not yet observed with the kernel client. Shale handles both (NET-005, NET-014).
- **Caches.** rclone's directory cache (1 s) and the FUSE attribute timeout (1 s) stand in for the kernel client's attribute cache (`actimeo`, 1 s by default); other
  clients' changes show up after about a second, inside Shale's 3 s rescan.

## Kernel mounts, to run by hand

The kernel CIFS and NFS mounts need root, so nothing runs them by itself. Run them once, record the `NET observation:` lines here, and
flip NET-011 and NET-012 to a test result (then close [#477](https://github.com/rabalyn/shale/issues/477)). Each stack is one at a time (the stacks share a
machine-wide lock); both leave nothing behind after `down`.

```sh
# SMB, kernel CIFS client (the server is rootless, only the mount asks for sudo)
tests/smb-env/smb-env.sh up
tests/smb-env/smb-env.sh mount                      # sudo mount -t cifs … -o vers=3.0; SHALE_SMB_ENV_VERS=2.1 mounts another protocol version
cargo test --bin shale -- --ignored smb_env --nocapture --test-threads=1 2>&1 | tee /tmp/smb-kernel.txt
tests/smb-env/smb-env.sh status                     # shows the cifs mount and its options
tests/smb-env/smb-env.sh down                       # sudo umount, stop the server

# NFS, kernel client against the kernel nfsd (the nfsd module must be loaded: sudo modprobe nfsd)
tests/nfs-env/nfs-env.sh up                         # sudo podman run … (privileged)
tests/nfs-env/nfs-env.sh mount                      # sudo mount -t nfs4 …; SHALE_NFS_ENV_OPTS=actimeo=60 (or noac, vers=3, nolock) for other options
cargo test --bin shale -- --ignored nfs_env --nocapture --test-threads=1 2>&1 | tee /tmp/nfs-kernel.txt
tests/nfs-env/nfs-env.sh down
```

`smb_env` matches `smb_env_storage_checks` and `smb_env_detect_…` (not the FUSE tests); `nfs_env` the two NFS tests. The NFS stack has **never been
started**: expect to fix details of the image's options on the first run. The interesting NFS runs repeat with `SHALE_NFS_ENV_OPTS=actimeo=60` and
`noac`. What to record: the observation lines (modification time granularity and skew, lock refused or granted, case and Unicode), the mount options
from `status`, and any failed assertion with its message. A failing assertion on the kernel client is a finding, not a test to weaken.

There is no rootless stand-in for NFS: rclone has no NFS client, and a FUSE NFS client (libnfs's `fuse-nfs`) is not packaged for most distributions,
would be built from source (a download of unpinned code), and would still need the kernel's nfsd on the other side, which needs root and a privileged
container anyway. `nfs_env_storage_checks` stays ignored, with that reason in its `#[ignore]` text.

## Limits of the evidence

- **The kernel SMB and NFS mounts did not run.** A kernel CIFS or NFS mount needs root (`sudo mount`); a user namespace cannot
  do it, and `mount.cifs` has no `/etc/fstab` entry. SMB ran through the rootless stand-in (rclone FUSE, a different client: see the section above);
  the kernel mounts are a manual run, prepared in "Kernel mounts, to run by hand" ([#477](https://github.com/rabalyn/shale/issues/477)). `nfs-env.sh`
  (server needs a privileged `sudo podman`) was written and syntax-checked, **never started**. The statements about the kernel CIFS client and about NFS
  above are therefore *documented*, not *ran*.
- Syncthing is one cloud-client model (event-driven, content-addressed, renames local files on conflict). Nextcloud,
  Dropbox, OneDrive and iCloud differ in how they pick up changes and in their placeholder files; their rows above are *documented*.
- The Syncthing tests exercise one pair on one Linux machine (ext4/btrfs-class timestamps).

## Inputs for #471 and #469

(Both are built: #471 as the file history, #469 as `gitsync/detect.rs` and the wizard, which use exactly these lists.)

**File-system type strings** (for the detection of network folders, warning only):

- Linux `statfs` / `/proc/self/mountinfo` fs type: `nfs`, `nfs4`, `cifs`, `smb3` (the kernel's newer name), `smbfs`,
  `ncpfs`, `9p`, `afs`, `ceph`, `glusterfs`, `lustre`, `davfs`/`fuse.davfs2`, and `fuse.*` generally (`fuse.sshfs`,
  `fuse.rclone`, `fuse.gvfsd-fuse`, `fuse.cryptomator`, `fuse.s3fs`; a plain `fuse` can be local, so a warning only).
  Magic numbers: NFS `0x6969`, SMB `0x517B`, CIFS `0xFF534D42`, SMB2 `0xFE534D42`.
- macOS `f_fstypename`: `nfs`, `smbfs`, `afpfs`, `webdav`, `webdavfs`, `osxfuse`/`macfuse`. Cloud folders on macOS
  live under `~/Library/CloudStorage/<Provider>-…` and `~/Library/Mobile Documents/` (iCloud), and are
  `apfs`: only the path tells.
- Windows: `GetDriveTypeW` = `DRIVE_REMOTE`, a UNC path `\\server\share`, or a mapped drive (`WNetGetConnection`).
  A OneDrive/Dropbox folder is a local NTFS folder with reparse-point placeholders
  (`FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS`, `FILE_ATTRIBUTE_RECALL_ON_OPEN`, `FILE_ATTRIBUTE_PINNED`/`UNPINNED`).
- Markers (as in #469): `.stfolder` (Syncthing; *ran*: present in both vault folders), `.dropbox`,
  `.dropbox.cache`, `.sync_*.db`/`.owncloudsync.log`/`.nextcloudsync.log` (Nextcloud/ownCloud), `.~lock.*`,
  `.tmp.driveupload` and `.tmp.drivedownload` (Google Drive), `.icloud` placeholder suffix (`.Name.ext.icloud`).

**Conflict-file name patterns** (to flag and to ignore in the history snapshots): see the table in section 5.
Shale matches them strictly (`conflict.rs`, NET-015); `Name-<COMPUTER>` (OneDrive), `Name 2` (iCloud) and
`Name (1)` (Google Drive) are too ambiguous to match by name.

**Temp and housekeeping names a snapshot (#471) and the scan must ignore**: `.shale-tmp-*` (Shale), `.syncthing.*.tmp`
and `.stfolder`/`.stversions`/`.stignore` (Syncthing), `.~lock.*#`, `~$*`, `*.tmp`, `*.part` (Office and Nextcloud),
`.nfs*` (NFS silly rename), `.fuse_hidden*`, `.icloud` stubs, `desktop.ini`, `.DS_Store`, `Thumbs.db`. Shale's own scan
already skips every dot name; a `~$Note.md` or a `Note.md.part` is not skipped by it.

**Polling**: needed, and already there: a rescan every 3 s on the UI thread; a watcher cannot see another machine's
change on NFS/SMB. A cloud client writes through the local file system, so a watcher would work for it, but one
mechanism for all is simpler. Required change: the scan off the UI thread, interval =
`max(3 s, 10 x duration of the last scan)` ([#473](https://github.com/rabalyn/shale/issues/473)); a snapshot on
"idle" (#471) can use the existing dirty/idle state of the window, not the scan. On NFS expect another machine's
change after up to `acdirmax` (60 s) unless `actimeo` is set.

**Snapshot design (#471)**: the `.git` of local-only history must be outside the vault (separate git-dir). A snapshot
commit must read file content (not trust mtime, which is coarse and skewed on shares) and must skip the
names above and `.shale/` device-local files (#476). Commit identity and timestamps come from the local clock; do
not compare them with the file's mtime.

**The wizard (#469)** says, in plain words (`gitsync/wizard/ui.rs`): "Nextcloud, Dropbox, OneDrive and Syncthing folders are safe for your
notes; do not put a `.git` folder in them"; the heuristic detection only warns. Concurrent edits are safe either way
(conflict copies, never an overwrite); the two things a user can notice are a `…conflict…` note and an old
tab layout after another device saved `.shale/state.json` (#476).
