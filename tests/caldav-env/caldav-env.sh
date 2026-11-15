#!/bin/sh
# Helper for Shale's CalDAV test environment. Usage: ./caldav-env.sh up | down | reset | status | logs
#   up      make the certificates (first time), start the containers, wait until they answer
#   down    stop and remove the containers (the calendars stay in the volume)
#   reset   down, and delete the volume too (the next up seeds the calendars again)
#   status  show the containers
#   logs    follow the containers' logs
# SHALE_CALDAV_ENV_COMPOSE overrides the compose command, e.g. SHALE_CALDAV_ENV_COMPOSE="docker compose".
set -eu
cd "$(dirname "$0")"

if [ -n "${SHALE_CALDAV_ENV_COMPOSE:-}" ]; then
  COMPOSE=$SHALE_CALDAV_ENV_COMPOSE
elif command -v podman-compose >/dev/null 2>&1; then
  COMPOSE=podman-compose
elif command -v docker >/dev/null 2>&1; then
  COMPOSE="docker compose"
else
  echo "neither podman-compose nor docker compose was found" >&2
  exit 1
fi

wait_for() { # host port seconds
  i=0
  while [ "$i" -lt "$3" ]; do
    if (exec 3<>"/dev/tcp/$1/$2") 2>/dev/null; then return 0; fi
    if command -v nc >/dev/null 2>&1 && nc -z "$1" "$2" 2>/dev/null; then return 0; fi
    i=$((i + 1))
    sleep 1
  done
  return 1
}

case "${1:-}" in
  up)
    ./setup-certs.sh
    $COMPOSE -f compose.yaml up -d
    wait_for 127.0.0.1 15232 90 || { echo "Radicale did not come up on 127.0.0.1:15232 (see: $0 logs)" >&2; exit 1; }
    echo "CalDAV test environment is up. See tests/caldav-env/README.md for the account."
    ;;
  down) $COMPOSE -f compose.yaml down ;;
  reset) $COMPOSE -f compose.yaml down -v ;;
  status) $COMPOSE -f compose.yaml ps ;;
  logs) $COMPOSE -f compose.yaml logs -f ;;
  *)
    echo "usage: $0 up | down | reset | status | logs" >&2
    exit 2
    ;;
esac
