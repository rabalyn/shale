#!/bin/sh
# Helper for Shale's NFS test environment (NET-012, #470): a real NFS server (the kernel's nfsd, in a privileged container) and the
# kernel's NFS client mounting its export, so that Shale's storage code runs on a real NFS mount. Usage:
#   ./nfs-env.sh up | down | status | mount | umount | dir | reset
#   up       start the server: container `shale-nfs-env` run with `sudo podman` (nfsd lives in the kernel: the container must be
#            privileged and a rootless one cannot be), export `/export` (NFSv4, `rw,sync,no_subtree_check,no_root_squash`)
#   down     unmount and stop the server
#   status   say whether the server runs and whether the export is mounted
#   mount    mount it on ${SHALE_NFS_ENV_MOUNT:-$XDG_RUNTIME_DIR/shale-nfs-env-mnt} with `sudo mount -t nfs4`; SHALE_NFS_ENV_OPTS=<opts>
#            adds mount options, e.g. `SHALE_NFS_ENV_OPTS=actimeo=60` or `nolock` or `vers=3`, to try how the attribute cache and the locks act
#   umount   unmount it again (sudo)
#   dir      print the mount point when the export is mounted here (exit 3 and a hint when it is not): `cargo test -- --ignored nfs_env`
#   reset    down, and forget the export's content
# Both steps need root: this stack cannot run in CI and is not part of .forgejo/scripts/env-tests.sh. NOT RUN YET: written on a
# machine where nothing here may use sudo (see docs/storage-on-shares.md and the issue named there).
# up, down, mount, umount and reset take the machine-wide lock of the podman stacks (tests/env-lock.sh, DEV-034).
set -eu
cd "$(dirname "$0")"
self=$(pwd)/$(basename "$0")

NAME=shale-nfs-env
IMAGE=docker.io/erichough/nfs-server:latest
MNT=${SHALE_NFS_ENV_MOUNT:-${XDG_RUNTIME_DIR:-/tmp}/shale-nfs-env-mnt-$(id -u)}
OPTS=${SHALE_NFS_ENV_OPTS:+,$SHALE_NFS_ENV_OPTS}
data=$(pwd)/data

case "${1:-}" in up|down|mount|umount|reset) . ../env-lock.sh; shale_env_lock ;; esac

running() { sudo podman container inspect -f '{{.State.Running}}' "$NAME" 2>/dev/null | grep -q true; }
mounted() { mountpoint -q "$MNT" 2>/dev/null; }

case "${1:-}" in
  up)
    if running; then echo "already up"; exit 0; fi
    sudo podman rm -f "$NAME" >/dev/null 2>&1 || true
    mkdir -p "$data/export"
    printf '/export *(rw,sync,no_subtree_check,no_root_squash,fsid=0)\n' > "$data/exports.txt"
    # the module nfsd must be loaded on the host (`modprobe nfsd`); the image mounts the nfsd file system itself
    sudo podman run -d --name "$NAME" --privileged -p 127.0.0.1:2049:2049 \
      -v "$data/export:/export" -v "$data/exports.txt:/etc/exports:ro" \
      -e NFS_VERSION=4 -e NFS_DISABLE_VERSION_3=1 "$IMAGE" >/dev/null 7>&-
    n=0
    until sudo podman logs "$NAME" 2>&1 | grep -q "Server startup complete"; do
      n=$((n + 1)); [ "$n" -lt 60 ] || { echo "nfsd does not come up" >&2; sudo podman logs "$NAME" >&2; exit 1; }
      sleep 0.5
    done
    echo "up: 127.0.0.1:/ (NFSv4 root = /export); now: $0 mount"
    ;;

  down)
    if mounted; then sh "$self" umount; fi
    sudo podman rm -f "$NAME" >/dev/null 2>&1 || true
    ;;

  status)
    if running; then echo "$NAME: running on 127.0.0.1:2049"; else echo "$NAME: not running"; fi
    if mounted; then echo "mounted on $MNT: $(findmnt -no FSTYPE,OPTIONS "$MNT" 2>/dev/null)"; else echo "not mounted ($MNT)"; fi
    ;;

  mount)
    running || { echo "the server is not up: run $0 up first" >&2; exit 1; }
    mounted && { echo "already mounted on $MNT"; exit 0; }
    mkdir -p "$MNT"
    echo "mounting 127.0.0.1:/ on $MNT with sudo (a kernel NFS mount needs root)"
    sudo mount -t nfs4 -o "port=2049$OPTS" 127.0.0.1:/ "$MNT"
    # the export belongs to root: hand the folder to the user the tests run as
    sudo chown "$(id -u):$(id -g)" "$MNT"
    ;;

  umount)
    if mounted; then sudo umount "$MNT"; fi
    ;;

  dir)
    if mounted; then echo "$MNT"; else echo "the NFS export is not mounted: run tests/nfs-env/nfs-env.sh up, then mount (needs sudo)" >&2; exit 3; fi
    ;;

  reset)
    sh "$self" down
    sudo rm -rf "$data"
    ;;

  *)
    echo "usage: $0 up | down | status | mount | umount | dir | reset" >&2
    exit 2
    ;;
esac
