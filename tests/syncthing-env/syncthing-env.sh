#!/bin/sh
# Helper for Shale's Syncthing test environment (NET-010, #470): two Syncthing containers that keep two host
# folders in sync, so that Shale's saves can be watched arriving on "the other device". Usage:
#   ./syncthing-env.sh up | down | status | dirs | pause | resume | reset
#   up       start both containers (rootless podman, a private network, no discovery, no relays, no
#            internet), wire them to each other and share the folder `shale-vault` (a rescan every 3 s,
#            the file watcher on with a 1 s delay); waits until both sides are connected
#   down     stop the containers (the folders stay)
#   status   say which containers run and whether the two devices are connected
#   dirs     print the two vault folders, A then B (two lines); the exit code is 3 when the stack is not up
#   pause    pause the folder on both devices (edits on both sides now conflict when it resumes)
#   resume   resume it
#   reset    down, then remove the containers' network and all the data (folders and configs)
# The data is in tests/syncthing-env/data/{a,b}/vault and {a,b}/config (not in git). The GUI/REST ports are
# 127.0.0.1:18384 (A) and 18385 (B), the API key is "shale-env".
# up, down, pause, resume and reset take the machine-wide lock of the podman stacks (tests/env-lock.sh, DEV-034).
# SHALE_SYNCTHING_ENV_ENGINE=docker selects Docker instead of Podman.
set -eu
# the containers must not inherit the lock (fd 7 of env-lock.sh): they outlive this script and would hold it
cd "$(dirname "$0")"
self=$(pwd)/$(basename "$0")
root=$(pwd)

ENGINE=${SHALE_SYNCTHING_ENV_ENGINE:-podman}
IMAGE=docker.io/syncthing/syncthing:2.0
NET=shale-syncthing-env
KEY=shale-env
FOLDER=shale-vault
data=$root/data

case "${1:-}" in up|down|pause|resume|reset) . ../env-lock.sh; shale_env_lock ;; esac

port_of() { if [ "$1" = a ]; then echo 18384; else echo 18385; fi; }
api() { # api <a|b> <METHOD> <path> [json]
  _s=$1; _m=$2; _p=$3; shift 3
  if [ $# -gt 0 ]; then
    curl -fsS -X "$_m" -H "X-API-Key: $KEY" -H 'Content-Type: application/json' -d "$1" "http://127.0.0.1:$(port_of "$_s")$_p"
  else
    curl -fsS -X "$_m" -H "X-API-Key: $KEY" "http://127.0.0.1:$(port_of "$_s")$_p"
  fi
}
running() { "$ENGINE" container inspect -f '{{.State.Running}}' "shale-st-$1" 2>/dev/null | grep -q true; }

wait_api() {
  _n=0
  until api "$1" GET /rest/system/ping >/dev/null 2>&1; do
    _n=$((_n + 1)); [ "$_n" -lt 120 ] || { echo "Syncthing $1 does not answer" >&2; "$ENGINE" logs "shale-st-$1" 2>&1 | tail -n 20 >&2; return 1; }
    sleep 0.5
  done
}

start_one() { # start_one <a|b>
  mkdir -p "$data/$1/vault" "$data/$1/config"
  "$ENGINE" run -d --name "shale-st-$1" --network "$NET" --network-alias "shale-st-$1" \
    --userns=keep-id --user "$(id -u):$(id -g)" \
    -e STGUIAPIKEY="$KEY" -e STNODEFAULTFOLDER=1 \
    -p "127.0.0.1:$(port_of "$1"):8384" \
    -v "$data/$1/vault:/vault" -v "$data/$1/config:/var/syncthing/config" \
    "$IMAGE" >/dev/null 7>&-
}

case "${1:-}" in
  up)
    if running a && running b; then echo "already up"; exit 0; fi
    "$ENGINE" rm -f shale-st-a shale-st-b >/dev/null 2>&1 || true
    "$ENGINE" network exists "$NET" 2>/dev/null || "$ENGINE" network create "$NET" >/dev/null
    start_one a; start_one b
    wait_api a; wait_api b
    ida=$(api a GET /rest/system/status | jq -r .myID)
    idb=$(api b GET /rest/system/status | jq -r .myID)
    for s in a b; do
      api $s PATCH /rest/config/options '{"globalAnnounceEnabled":false,"localAnnounceEnabled":false,"relaysEnabled":false,"natEnabled":false,"urAccepted":-1,"startBrowser":false}' >/dev/null
      api $s DELETE /rest/config/folders/default >/dev/null 2>&1 || true
    done
    api a POST /rest/config/devices "{\"deviceID\":\"$idb\",\"name\":\"b\",\"addresses\":[\"tcp://shale-st-b:22000\"],\"autoAcceptFolders\":false}" >/dev/null
    api b POST /rest/config/devices "{\"deviceID\":\"$ida\",\"name\":\"a\",\"addresses\":[\"tcp://shale-st-a:22000\"],\"autoAcceptFolders\":false}" >/dev/null
    folder() { # folder <side> <other id>
      printf '{"id":"%s","label":"vault","path":"/vault","type":"sendreceive","rescanIntervalS":3,"fsWatcherEnabled":true,"fsWatcherDelayS":1,"devices":[{"deviceID":"%s"}]}' "$FOLDER" "$2"
    }
    api a POST /rest/config/folders "$(folder a "$idb")" >/dev/null
    api b POST /rest/config/folders "$(folder b "$ida")" >/dev/null
    n=0
    until [ "$(api a GET /rest/system/connections | jq -r --arg d "$idb" '.connections[$d].connected')" = true ]; do
      n=$((n + 1)); [ "$n" -lt 120 ] || { echo "the two Syncthing devices do not connect" >&2; exit 1; }
      sleep 0.5
    done
    echo "up: A $data/a/vault, B $data/b/vault"
    ;;

  down)
    "$ENGINE" rm -f shale-st-a shale-st-b >/dev/null 2>&1 || true
    ;;

  status)
    for s in a b; do
      if running $s; then echo "shale-st-$s: running (GUI http://127.0.0.1:$(port_of $s))"; else echo "shale-st-$s: not running"; fi
    done
    if running a && running b; then
      api a GET /rest/system/connections | jq -r '.connections | to_entries[] | "A sees device \(.key[0:7]): connected=\(.value.connected)"'
    fi
    ;;

  dirs)
    if running a && running b; then printf "%s\n%s\n" "$data/a/vault" "$data/b/vault"; else echo "the Syncthing stack is not up: run tests/syncthing-env/syncthing-env.sh up" >&2; exit 3; fi
    ;;

  pause|resume)
    flag=true; [ "$1" = resume ] && flag=false
    for s in a b; do api $s PATCH "/rest/config/folders/$FOLDER" "{\"paused\":$flag}" >/dev/null; done
    ;;

  reset)
    sh "$self" down
    "$ENGINE" network rm "$NET" >/dev/null 2>&1 || true
    # the files were made by the container's user, which is ours (keep-id): a plain rm works
    rm -rf "$data"
    ;;

  *)
    echo "usage: $0 up | down | status | dirs | pause | resume | reset" >&2
    exit 2
    ;;
esac
