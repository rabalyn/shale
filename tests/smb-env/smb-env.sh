#!/bin/sh
# Helper for Shale's Samba test environment (NET-011, #470): a real Samba server (smbd) in a rootless container, and the
# kernel's own CIFS client mounting its share, so that Shale's storage code runs on a real SMB share. Usage:
#   ./smb-env.sh up | down | status | mount | umount | dir | fuse-mount | fuse-umount | fuse-dir | reset
#   up       start the server (container `shale-smb-env`, 127.0.0.1:1445, share `vault`, user shale / password shale); rootless
#   down     unmount and stop the server
#   status   say whether the server runs and whether the share is mounted
#   mount    mount the share on ${SHALE_SMB_ENV_MOUNT:-$XDG_RUNTIME_DIR/shale-smb-env-mnt} with `sudo mount -t cifs`: THE ONE STEP THAT
#            NEEDS ROOT. A rootless user cannot kernel-mount CIFS (not inside a user namespace either), and mount.cifs is no help
#            without an /etc/fstab line. Nothing runs this by itself: you do, once, and `sudo` asks you.
#   umount   unmount it again (sudo)
#   dir      print the mount point when the share is mounted here (exit 3 and a hint when it is not): `cargo test -- --ignored smb_env`
#            uses it
#   fuse-mount   the ROOTLESS STAND-IN (#477): mount the share with `rclone mount` (a FUSE client that speaks SMB) on
#            ${SHALE_SMB_ENV_FUSE_MOUNT:-$XDG_RUNTIME_DIR/shale-smb-env-fuse-<uid>}, no root needed (/dev/fuse and fusermount3). It is NOT the
#            kernel CIFS client: what it shows is rclone's SMB client and its VFS layer, so the observations carry that label
#            (`smb_fuse_env_storage_checks`). rclone is a system `rclone` when there is one, else the pinned image
#            docker.io/rclone/rclone:1.70.3, whose binary is copied (`create` + `cp`, no download of a loose binary) to
#            ${XDG_CACHE_HOME:-~/.cache}/shale-dev/rclone-1.70.3/rclone. The config is made from RCLONE_CONFIG_SHALESMB_* variables
#            (the password obscured with `rclone obscure`), never written to disk. Options, and why:
#              --vfs-cache-mode writes   a file opened for writing is buffered in a local cache and uploaded when it is closed; this is
#                                        what lets an in-place write and Shale's temp-file-then-rename save work (mode `off` cannot
#                                        write at an offset), while reads are not cached (mode `full` would hide a change of another
#                                        client until its cache expires; the kernel client, cache=strict, does not cache file data
#                                        across opens either)
#              --vfs-write-back 0s       upload a closed file at once (default 5 s): with the default, a change in place of the same
#                                        length shows the old mtime until the upload (NET-002 saw the change only seconds later); the
#                                        kernel client writes through at close
#              --dir-cache-time 1s       how long a directory listing is kept (default 5 m): other clients' new files show up within a
#                                        second, like Shale's 3 s rescan expects (the kernel client's actimeo is 1 s)
#              --attr-timeout 1s         how long the kernel keeps file attributes (default 1 s, set so it is written down)
#              --vfs-cache-max-age 1m    a closed file does not stay in the write cache
#              --daemon-wait 30s         the command returns when the mount is up, or fails with the reason
#   fuse-umount  `fusermount3 -u` and remove the cache folder
#   fuse-dir print the FUSE mount point when it is mounted (exit 3 and a hint when it is not)
#   reset    down, and forget the share's content
# up, down, mount, umount, fuse-mount, fuse-umount and reset take the machine-wide lock of the podman stacks (tests/env-lock.sh, DEV-034).
# SHALE_SMB_ENV_ENGINE=docker selects Docker instead of Podman; SHALE_SMB_ENV_VERS=<n> the SMB protocol version (default 3.0).
set -eu
cd "$(dirname "$0")"
self=$(pwd)/$(basename "$0")

ENGINE=${SHALE_SMB_ENV_ENGINE:-podman}
NAME=shale-smb-env
IMAGE=docker.io/dperson/samba:latest
PORT=1445
MNT=${SHALE_SMB_ENV_MOUNT:-${XDG_RUNTIME_DIR:-/tmp}/shale-smb-env-mnt-$(id -u)}
FMNT=${SHALE_SMB_ENV_FUSE_MOUNT:-${XDG_RUNTIME_DIR:-/tmp}/shale-smb-env-fuse-$(id -u)}
FCACHE=${XDG_RUNTIME_DIR:-/tmp}/shale-smb-env-fuse-cache-$(id -u)
RCLONE_VERSION=1.70.3
RCLONE_IMAGE=docker.io/rclone/rclone:$RCLONE_VERSION

case "${1:-}" in up|down|mount|umount|fuse-mount|fuse-umount|reset) . ../env-lock.sh; shale_env_lock ;; esac

running() { "$ENGINE" container inspect -f '{{.State.Running}}' "$NAME" 2>/dev/null | grep -q true; }
mounted() { mountpoint -q "$MNT" 2>/dev/null; }
fmounted() { mountpoint -q "$FMNT" 2>/dev/null; }

# the rclone to use: SHALE_SMB_ENV_RCLONE, a system rclone, or the binary of the pinned image (cached)
rclone_bin() {
  if [ -n "${SHALE_SMB_ENV_RCLONE:-}" ]; then echo "$SHALE_SMB_ENV_RCLONE"; return; fi
  if command -v rclone >/dev/null 2>&1; then command -v rclone; return; fi
  _bin=${XDG_CACHE_HOME:-$HOME/.cache}/shale-dev/rclone-$RCLONE_VERSION/rclone
  if [ ! -x "$_bin" ]; then
    mkdir -p "$(dirname "$_bin")"
    echo "taking rclone $RCLONE_VERSION out of $RCLONE_IMAGE" >&2
    "$ENGINE" pull -q "$RCLONE_IMAGE" >/dev/null
    _id=$("$ENGINE" create "$RCLONE_IMAGE")
    "$ENGINE" cp "$_id:/usr/local/bin/rclone" "$_bin.part" >/dev/null
    "$ENGINE" rm "$_id" >/dev/null
    chmod +x "$_bin.part"
    "$_bin.part" version | head -n 1 | grep -q "v$RCLONE_VERSION" || { echo "the rclone of $RCLONE_IMAGE is not $RCLONE_VERSION" >&2; rm -f "$_bin.part"; exit 1; }
    mv "$_bin.part" "$_bin"
  fi
  echo "$_bin"
}

case "${1:-}" in
  up)
    if running; then echo "already up"; exit 0; fi
    "$ENGINE" rm -f "$NAME" >/dev/null 2>&1 || true
    # a share `vault` at /share, read-write, owned by the user shale (-p: the owner of a new file is the user, not root)
    "$ENGINE" run -d --name "$NAME" -p "127.0.0.1:$PORT:445" "$IMAGE" -u "shale;shale" -s "vault;/share;yes;no;no;shale" -p >/dev/null 7>&-
    n=0
    until "$ENGINE" logs "$NAME" 2>&1 | grep -q "ready to serve connections"; do
      n=$((n + 1)); [ "$n" -lt 60 ] || { echo "smbd does not come up" >&2; "$ENGINE" logs "$NAME" >&2; exit 1; }
      sleep 0.5
    done
    echo "up: //127.0.0.1:$PORT/vault (shale / shale); now: $0 fuse-mount (rootless, rclone) or $0 mount (kernel CIFS, sudo)"
    ;;

  down)
    if mounted; then sh "$self" umount; fi
    if fmounted; then sh "$self" fuse-umount; fi
    "$ENGINE" rm -f "$NAME" >/dev/null 2>&1 || true
    ;;

  status)
    if running; then echo "$NAME: running on 127.0.0.1:$PORT"; else echo "$NAME: not running"; fi
    if mounted; then echo "mounted on $MNT: $(findmnt -no FSTYPE,OPTIONS "$MNT" 2>/dev/null)"; else echo "not mounted ($MNT)"; fi
    if fmounted; then echo "FUSE-mounted (rclone) on $FMNT"; else echo "not FUSE-mounted ($FMNT)"; fi
    ;;

  mount)
    running || { echo "the server is not up: run $0 up first" >&2; exit 1; }
    mounted && { echo "already mounted on $MNT"; exit 0; }
    mkdir -p "$MNT"
    echo "mounting //127.0.0.1/vault on $MNT with sudo (a kernel CIFS mount needs root)"
    sudo mount -t cifs //127.0.0.1/vault "$MNT" -o "port=$PORT,vers=${SHALE_SMB_ENV_VERS:-3.0},username=shale,password=shale,uid=$(id -u),gid=$(id -g),file_mode=0644,dir_mode=0755"
    ;;

  umount)
    if mounted; then sudo umount "$MNT"; fi
    ;;

  dir)
    if mounted; then echo "$MNT"; else echo "the Samba share is not mounted: run tests/smb-env/smb-env.sh up, then mount (needs sudo)" >&2; exit 3; fi
    ;;

  fuse-mount)
    running || { echo "the server is not up: run $0 up first" >&2; exit 1; }
    fmounted && { echo "already FUSE-mounted on $FMNT"; exit 0; }
    [ -c /dev/fuse ] && command -v fusermount3 >/dev/null 2>&1 || { echo "no /dev/fuse or no fusermount3 here: cannot FUSE-mount" >&2; exit 1; }
    rclone=$(rclone_bin)
    mkdir -p "$FMNT" "$FCACHE"
    echo "FUSE-mounting //127.0.0.1:$PORT/vault on $FMNT with rclone (NOT the kernel CIFS client): $("$rclone" version | head -n 1)"
    # 7>&-: the daemon must not keep the machine-wide lock of the stacks (fd 7, tests/env-lock.sh) after this command ends
    RCLONE_CONFIG_SHALESMB_TYPE=smb RCLONE_CONFIG_SHALESMB_HOST=127.0.0.1 RCLONE_CONFIG_SHALESMB_PORT=$PORT \
      RCLONE_CONFIG_SHALESMB_USER=shale RCLONE_CONFIG_SHALESMB_PASS=$("$rclone" obscure shale) RCLONE_CONFIG=/dev/null \
      "$rclone" mount SHALESMB:vault "$FMNT" --daemon --daemon-wait 30s \
        --vfs-cache-mode writes --vfs-cache-max-age 1m --vfs-write-back 0s --dir-cache-time 1s --attr-timeout 1s \
        --cache-dir "$FCACHE" --log-file "$FCACHE/rclone.log" --log-level NOTICE 7>&-
    fmounted || { echo "the rclone mount did not come up; its log:" >&2; cat "$FCACHE/rclone.log" >&2 2>/dev/null || true; exit 1; }
    echo "mounted: $FMNT"
    ;;

  fuse-umount)
    if fmounted; then fusermount3 -u "$FMNT"; fi
    rm -rf "$FCACHE"
    rmdir "$FMNT" 2>/dev/null || true
    ;;

  fuse-dir)
    if fmounted; then echo "$FMNT"; else echo "the Samba share is not FUSE-mounted: run tests/smb-env/smb-env.sh up, then fuse-mount" >&2; exit 3; fi
    ;;

  reset)
    sh "$self" down
    ;;

  *)
    echo "usage: $0 up | down | status | mount | umount | dir | fuse-mount | fuse-umount | fuse-dir | reset" >&2
    exit 2
    ;;
esac
