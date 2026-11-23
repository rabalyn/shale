#!/bin/sh
# Helper for Shale's mail test environment. Usage: ./mail-env.sh up | down | reset | revoke | status | logs
#   up      make the certificates (first time), start the containers, wait until they answer
#   down    stop and remove the containers (the mail stays in the volume)
#   reset   down, and delete the volume too (the next up seeds the mailboxes again)
#   revoke  restart the OAuth2 provider: it forgets its signing key and refresh tokens, so every
#           token handed out before is dead (to try "the sign-in was revoked" by hand)
#   status  show the containers
#   logs    follow the containers' logs
# up, down, reset and revoke take the machine-wide lock of the podman stacks (tests/env-lock.sh, DEV-034): two checkouts take turns.
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

# waits until IMAPS answers with a real IMAP greeting through TLS (an open port is not enough: the
# published port accepts connections before Dovecot listens behind it); gives up after $1 seconds
wait_for_imap() { # seconds
  i=0
  while [ "$i" -lt "$1" ]; do
    greeting=$(printf 'a logout\r\n' | timeout 5 openssl s_client -quiet -connect 127.0.0.1:10993 2>/dev/null | head -n 1 || true)
    case "$greeting" in "* OK"*) return 0 ;; esac
    i=$((i + 1))
    sleep 1
  done
  return 1
}

# no container started below may keep the lock's file descriptor (7): a daemon that outlived the script would hold the lock
case "${1:-}" in up|down|reset|revoke) . ../env-lock.sh; shale_env_lock ;; esac

case "${1:-}" in
  up)
    ./setup-certs.sh
    $COMPOSE -f compose.yaml up -d 7>&-
    wait_for 127.0.0.1 10993 90 || { echo "Dovecot did not come up on 127.0.0.1:10993 (see: $0 logs)" >&2; exit 1; }
    wait_for 127.0.0.1 18080 90 || { echo "the OAuth2 provider did not come up on 127.0.0.1:18080 (see: $0 logs)" >&2; exit 1; }
    wait_for 127.0.0.1 18025 90 || { echo "the mail sink (Mailpit) did not come up on 127.0.0.1:18025 (see: $0 logs)" >&2; exit 1; }
    echo "Mail test environment is up. See tests/mail-env/README.md for the accounts."
    ;;
  down) $COMPOSE -f compose.yaml down 7>&- ;;
  reset) $COMPOSE -f compose.yaml down -v 7>&- ;;
  revoke)
    # Dovecot and the sink share the provider's network, so they are stopped first and started last
    $COMPOSE -f compose.yaml stop dovecot mailpit oauth 7>&-
    $COMPOSE -f compose.yaml start oauth 7>&-
    wait_for 127.0.0.1 18080 60 || { echo "the OAuth2 provider did not come back" >&2; exit 1; }
    $COMPOSE -f compose.yaml start mailpit 7>&-
    $COMPOSE -f compose.yaml start dovecot 7>&-
    wait_for 127.0.0.1 10993 60 || { echo "Dovecot did not come back" >&2; exit 1; }
    wait_for 127.0.0.1 18025 60 || { echo "the mail sink did not come back" >&2; exit 1; }
    wait_for_imap 30 || { echo "Dovecot did not greet with IMAP within 30 s (see: $0 logs)" >&2; exit 1; }
    echo "The provider was restarted: all tokens it handed out are invalid now."
    ;;
  status) $COMPOSE -f compose.yaml ps ;;
  logs) $COMPOSE -f compose.yaml logs -f ;;
  *)
    echo "usage: $0 up | down | reset | revoke | status | logs" >&2
    exit 2
    ;;
esac
