#!/bin/sh
# Helper for Shale's Forgejo test environment (SYNC-066): a real Forgejo (SQLite) on 127.0.0.1:13000 with a
# seeded administrator. Usage: ./forgejo-env.sh up | down | reset | status | logs
#   up      start the container, wait until it answers, make the administrator (the first time)
#   down    stop and remove the container (the data stays in the volume)
#   reset   down, and delete the volumes too (the next up makes a fresh Forgejo)
#   status  show the container
#   logs    follow the container's log
#   runner-up <folder>   start a Forgejo Actions runner (runner.yaml, SITE-049, #665) whose /srv/www is <folder> and wait
#                        until Forgejo has it; the first time it builds the runner image (Debian with Node.js, git,
#                        git-lfs and curl; the jobs run in it, no nested containers); a running runner is replaced
#   job-log <owner/repo> print the log of the Actions jobs of that repository (needs zstd)
#   runner-down          stop and remove the runner (down and reset do it too)
# The account: user `shale`, password `shale-forgejo-test-1` (a test account of a throw-away server that listens
# on 127.0.0.1 only; the tests make their own tokens through the API).
# up, down and reset take the machine-wide lock of the podman stacks (tests/env-lock.sh, DEV-034): two checkouts take turns.
# SHALE_FORGEJO_ENV_COMPOSE overrides the compose command, e.g. SHALE_FORGEJO_ENV_COMPOSE="docker compose".
set -eu
cd "$(dirname "$0")"

if [ -n "${SHALE_FORGEJO_ENV_COMPOSE:-}" ]; then
  COMPOSE=$SHALE_FORGEJO_ENV_COMPOSE
elif command -v podman-compose >/dev/null 2>&1; then
  COMPOSE=podman-compose
elif command -v docker >/dev/null 2>&1; then
  COMPOSE="docker compose"
else
  echo "neither podman-compose nor docker compose was found" >&2
  exit 1
fi

URL=http://127.0.0.1:13000
USER_NAME=shale
USER_PASSWORD=shale-forgejo-test-1

# wait_http <seconds>: until Forgejo's health check answers
wait_http() {
  i=0
  while [ "$i" -lt "$1" ]; do
    if curl -q -sf -o /dev/null "$URL/api/healthz" 2>/dev/null; then return 0; fi
    i=$((i + 1))
    sleep 1
  done
  return 1
}

# no container started below may keep the lock's file descriptor (7): a daemon that outlived the script would hold the lock
case "${1:-}" in up|down|reset) . ../env-lock.sh; shale_env_lock ;; esac

case "${1:-}" in
  up)
    $COMPOSE -f compose.yaml up -d 7>&-
    wait_http 120 || { echo "Forgejo did not come up on $URL (see: $0 logs)" >&2; exit 1; }
    # the administrator, once (a second run finds it and goes on)
    if ! curl -q -sf -o /dev/null -u "$USER_NAME:$USER_PASSWORD" "$URL/api/v1/user"; then
      $COMPOSE -f compose.yaml exec -T forgejo forgejo admin user create --admin --username "$USER_NAME" --password "$USER_PASSWORD" --email shale@example.test --must-change-password=false 7>&-
    fi
    curl -q -sf -o /dev/null -u "$USER_NAME:$USER_PASSWORD" "$URL/api/v1/user" || { echo "the administrator cannot sign in" >&2; exit 1; }
    echo "Forgejo test environment is up on $URL. See tests/forgejo-env/README.md for the account."
    ;;
  down)
    SHALE_FORGEJO_WWW=/tmp $COMPOSE -f runner.yaml down 7>&- 2>/dev/null || true
    $COMPOSE -f compose.yaml down 7>&-
    ;;
  reset)
    SHALE_FORGEJO_WWW=/tmp $COMPOSE -f runner.yaml down 7>&- 2>/dev/null || true
    $COMPOSE -f compose.yaml down -v 7>&-
    ;;
  runner-up)
    [ -n "${2:-}" ] || { echo "usage: $0 runner-up <folder for /srv/www>" >&2; exit 2; }
    mkdir -p "$2"
    SHALE_FORGEJO_WWW=$(cd "$2" && pwd) $COMPOSE -f runner.yaml up -d --build --force-recreate 7>&-
    i=0
    until SHALE_FORGEJO_WWW=/tmp $COMPOSE -f runner.yaml logs runner 2>&1 | grep -q "declared successfully"; do
      i=$((i + 1))
      [ "$i" -lt 60 ] || { echo "the runner did not register with Forgejo (see: $COMPOSE -f runner.yaml logs)" >&2; exit 1; }
      sleep 1
    done
    echo "The Actions runner is up; /srv/www of its jobs is $2"
    ;;
  job-log)
    # the log of the Actions jobs of a repository (owner/name), for a test that failed; needs zstd on this machine
    [ -n "${2:-}" ] || { echo "usage: $0 job-log <owner/repo>" >&2; exit 2; }
    $COMPOSE -f compose.yaml exec -T forgejo sh -c "cat /var/lib/gitea/actions_log/$2/*/*.log.zst" | zstd -dc
    ;;
  runner-down) SHALE_FORGEJO_WWW=/tmp $COMPOSE -f runner.yaml down 7>&- ;;
  status) $COMPOSE -f compose.yaml ps ;;
  logs) $COMPOSE -f compose.yaml logs -f ;;
  *)
    echo "usage: $0 up | down | reset | status | logs | runner-up <folder> | runner-down | job-log <owner/repo>" >&2
    exit 2
    ;;
esac
