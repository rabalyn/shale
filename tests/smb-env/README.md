# Samba test environment

A real Samba server (`docker.io/dperson/samba`, smbd 4.12) in a rootless container, share `vault`, user `shale`, password `shale`,
on 127.0.0.1:1445. Shale's storage checks (`src/netshare_env_tests.rs`, NET-006, NET-011, NET-018,
[`docs/storage-on-shares.md`](../../docs/storage-on-shares.md), #470, #477) run in the share once it is mounted. Two ways to mount it:

## Rootless: rclone over FUSE (a stand-in, NOT the kernel CIFS client)

```sh
tests/smb-env/smb-env.sh up
tests/smb-env/smb-env.sh fuse-mount     # rclone mount on $XDG_RUNTIME_DIR/shale-smb-env-fuse-<uid>; waits until it is up
cargo test --bin shale -- --ignored smb_fuse_env --nocapture --test-threads=1    # prints the `NET observation` lines
tests/smb-env/smb-env.sh down           # fusermount3 -u, stop the server
```

Needs `/dev/fuse` and `fusermount3` (setuid), nothing else. rclone is a system `rclone` when there is one (or `SHALE_SMB_ENV_RCLONE`), else
the binary of the pinned image `docker.io/rclone/rclone:1.70.3`, copied out with `create` and `cp` into
`${XDG_CACHE_HOME:-~/.cache}/shale-dev/rclone-1.70.3/rclone` and checked for its version. Its config is made from `RCLONE_CONFIG_SHALESMB_*`
environment variables (the password obscured with `rclone obscure`) and never written to disk. The mount options and the reason for each are in the
header of `smb-env.sh` (`--vfs-cache-mode writes`, `--vfs-write-back 0s`, `--dir-cache-time 1s`, `--attr-timeout 1s`).

Every observation of this run is labelled "rclone FUSE, not the kernel CIFS client": it is rclone's SMB client and its VFS layer, so locks
(none reach the server), write-back timing and the names it folds differ from the kernel's. What it shows is Shale's own code on a share served
by a real smbd. `.forgejo/scripts/env-tests.sh run` runs it as the suite `smb_fuse_env` (also for `scripts/land.sh`); where `/dev/fuse` or
`fusermount3` is missing, which is the case in the Forgejo job container, it says "SKIPPED" and leaves the suite out.

## The kernel CIFS client (needs root, by hand)

A rootless user cannot mount CIFS with the kernel client (not in a user namespace either), and mount.cifs is no help without an
`/etc/fstab` line, so the owner mounts the share once with `sudo`:

```sh
tests/smb-env/smb-env.sh up
tests/smb-env/smb-env.sh mount    # sudo mount -t cifs … on $XDG_RUNTIME_DIR/shale-smb-env-mnt-<uid>  (SHALE_SMB_ENV_VERS=2.1 for another protocol version)
cargo test --bin shale -- --ignored smb_env --nocapture --test-threads=1    # prints the `NET observation` lines
tests/smb-env/smb-env.sh down     # umount (sudo) and stop
```

Not run yet: the ignored test says so ("the Samba share is not mounted") without the mount. It is not in `.forgejo/scripts/env-tests.sh` and cannot be in
CI without a privileged runner (rabalyn/shale#477); the exact steps for the manual run, and where to record it, are in
[`docs/storage-on-shares.md`](../../docs/storage-on-shares.md), "Kernel mounts, to run by hand".
