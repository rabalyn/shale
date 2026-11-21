#!/bin/sh
# Helper for Shale's Samba test environment (NET-011, #470): a real Samba server (smbd) in a rootless container, and the
# kernel's own CIFS client mounting its share, so that Shale's storage code runs on a real SMB share. Usage:
#   ./smb-env.sh up | down | status | mount | umount | dir | reset
#   up       start the server (container `shale-smb-env`, 127.0.0.1:1445, share `vault`, user shale / password shale); rootless
#   down     unmount and stop the server
#   status   say whether the server runs and whether the share is mounted
#   mount    mount the share on ${SHALE_SMB_ENV_MOUNT:-$XDG_RUNTIME_DIR/shale-smb-env-mnt} with `sudo mount -t cifs`: THE ONE STEP THAT
#            NEEDS ROOT. A rootless user cannot kernel-mount CIFS (not inside a user namespace either), and mount.cifs is no help
#            without an /etc/fstab line. Nothing runs this by itself: you do, once, and `sudo` asks you.
#   umount   unmount it again (sudo)
#   dir      print the mount point when the share is mounted here (exit 3 and a hint when it is not): `cargo test -- --ignored smb_env`
#            uses it
#   reset    down, and forget the share's content
# up, down, mount, umount and reset take the machine-wide lock of the podman stacks (tests/env-lock.sh, DEV-034).
# SHALE_SMB_ENV_ENGINE=docker selects Docker instead of Podman; SHALE_SMB_ENV_VERS=<n> the SMB protocol version (default 3.0).
set -eu
cd "$(dirname "$0")"
self=$(pwd)/$(basename "$0")

ENGINE=${SHALE_SMB_ENV_ENGINE:-podman}
NAME=shale-smb-env
IMAGE=docker.io/dperson/samba:latest
PORT=1445
MNT=${SHALE_SMB_ENV_MOUNT:-${XDG_RUNTIME_DIR:-/tmp}/shale-smb-env-mnt-$(id -u)}

case "${1:-}" in up|down|mount|umount|reset) . ../env-lock.sh; shale_env_lock ;; esac

running() { "$ENGINE" container inspect -f '{{.State.Running}}' "$NAME" 2>/dev/null | grep -q true; }
mounted() { mountpoint -q "$MNT" 2>/dev/null; }

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
    echo "up: //127.0.0.1:$PORT/vault (shale / shale); now: $0 mount"
    ;;

  down)
    if mounted; then sh "$self" umount; fi
    "$ENGINE" rm -f "$NAME" >/dev/null 2>&1 || true
    ;;

  status)
    if running; then echo "$NAME: running on 127.0.0.1:$PORT"; else echo "$NAME: not running"; fi
    if mounted; then echo "mounted on $MNT: $(findmnt -no FSTYPE,OPTIONS "$MNT" 2>/dev/null)"; else echo "not mounted ($MNT)"; fi
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

  reset)
    sh "$self" down
    ;;

  *)
    echo "usage: $0 up | down | status | mount | umount | dir | reset" >&2
    exit 2
    ;;
esac
