#!/bin/sh
# Helper for Shale's Forgejo test environment (SYNC-066): a real Forgejo (SQLite) on 127.0.0.1:13000 with a
# seeded administrator. Usage: ./forgejo-env.sh up | down | reset | status | logs
#   up      start the container, wait until it answers, make the administrator (the first time)
#   down    stop and remove the container (the data stays in the volume)
#   reset   down, and delete the volumes too (the next up makes a fresh Forgejo)
#   status  show the container
#   logs    follow the container's log
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
  down) $COMPOSE -f compose.yaml down 7>&- ;;
  reset) $COMPOSE -f compose.yaml down -v 7>&- ;;
  status) $COMPOSE -f compose.yaml ps ;;
  logs) $COMPOSE -f compose.yaml logs -f ;;
  *)
    echo "usage: $0 up | down | reset | status | logs" >&2
    exit 2
    ;;
esac
