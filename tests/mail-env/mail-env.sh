#!/bin/sh
# Helper for Shale's mail test environment. Usage: ./mail-env.sh up | down | reset | revoke | status | logs
#   up      make the certificates (first time), start the containers, wait until they answer
#   down    stop and remove the containers (the mail stays in the volume)
#   reset   down, and delete the volume too (the next up seeds the mailboxes again)
#   revoke  restart the OAuth2 provider: it forgets its signing key and refresh tokens, so every
#           token handed out before is dead (to try "the sign-in was revoked" by hand)
#   status  show the containers
#   logs    follow the containers' logs
# SHALE_MAIL_ENV_COMPOSE overrides the compose command, e.g. SHALE_MAIL_ENV_COMPOSE="docker compose".
set -eu
cd "$(dirname "$0")"

if [ -n "${SHALE_MAIL_ENV_COMPOSE:-}" ]; then
  COMPOSE=$SHALE_MAIL_ENV_COMPOSE
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
    # /dev/tcp is not in every sh: fall back to a tool
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
    wait_for 127.0.0.1 10993 90 || { echo "Dovecot did not come up on 127.0.0.1:10993 (see: $0 logs)" >&2; exit 1; }
    wait_for 127.0.0.1 18080 90 || { echo "the OAuth2 provider did not come up on 127.0.0.1:18080 (see: $0 logs)" >&2; exit 1; }
    wait_for 127.0.0.1 18025 90 || { echo "the mail sink (Mailpit) did not come up on 127.0.0.1:18025 (see: $0 logs)" >&2; exit 1; }
    echo "Mail test environment is up. See tests/mail-env/README.md for the accounts."
    ;;
  down) $COMPOSE -f compose.yaml down ;;
  reset) $COMPOSE -f compose.yaml down -v ;;
  revoke)
    # Dovecot and the sink share the provider's network, so they are stopped first and started last
    $COMPOSE -f compose.yaml stop dovecot mailpit oauth
    $COMPOSE -f compose.yaml start oauth
    wait_for 127.0.0.1 18080 60 || { echo "the OAuth2 provider did not come back" >&2; exit 1; }
    $COMPOSE -f compose.yaml start mailpit
    $COMPOSE -f compose.yaml start dovecot
    wait_for 127.0.0.1 10993 60 || { echo "Dovecot did not come back" >&2; exit 1; }
    wait_for 127.0.0.1 18025 60 || { echo "the mail sink did not come back" >&2; exit 1; }
    sleep 2
    echo "The provider was restarted: all tokens it handed out are invalid now."
    ;;
  status) $COMPOSE -f compose.yaml ps ;;
  logs) $COMPOSE -f compose.yaml logs -f ;;
  *)
    echo "usage: $0 up | down | reset | revoke | status | logs" >&2
    exit 2
    ;;
esac
