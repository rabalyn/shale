#!/bin/sh
# The CI job "env-tests" (DEV-005, CALENV-004): runs the real-server suites of the CalDAV and mail
# test environments. Usage:
#   env-tests.sh wanted <event> <base-sha> <head-sha>
#       prints "yes" or "no": a pull request only runs the job when it touches the code or the
#       environments (a push to main and the weekly run always do)
#   env-tests.sh detect
#       prints "podman", "docker" or "none": which container engine the job container can use
#   env-tests.sh run [podman|docker]
#       uses the given engine (the one the detect step found; detected again when left out),
#       starts both stacks, runs `cargo test -- --ignored caldav_env` and `… mail_env`, and always
#       takes the stacks down again; exits 1 if a stack or a test fails or no test ran
set -eu
cd "$(dirname "$0")/../.."

case "${1:-}" in
  wanted)
    if [ "${2:-}" != pull_request ]; then echo yes; exit 0; fi
    files=$(git diff --name-only "$3" "$4")
    if printf '%s\n' "$files" | grep -Eq '^(src/(cal_|caldav_|imap|mail|smtp)|tests/(caldav|mail)-env/|\.forgejo/(workflows/ci\.yml|scripts/env-tests\.sh))'; then
      echo yes
    else
      echo no
    fi
    ;;

  detect)
    # `info` alone is not enough: an unprivileged job container answers it but cannot start a
    # container (no subordinate user ids, see tests/caldav-env/README.md "In CI"), so start one
    probe=docker.io/library/busybox:1.36
    if command -v podman >/dev/null 2>&1 && podman info >/dev/null 2>&1 && podman run --rm "$probe" true >/dev/null 2>&1; then
      echo podman
    elif command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1 && docker run --rm "$probe" true >/dev/null 2>&1; then
      echo docker
    else
      echo none
      # say why, for the log (stderr, so the answer on stdout stays one word)
      if command -v podman >/dev/null 2>&1; then podman run --rm "$probe" true 2>&1 | tail -n 5 >&2 || true; fi
    fi
    ;;

  run)
    # a second `podman info` right after the first can fail where the first worked: trust the
    # engine the detect step found, and let the stacks themselves show a real problem
    engine=${2:-$(sh "$0" detect)}
    case $engine in
      podman) ;;
      docker)
        SHALE_CALDAV_ENV_COMPOSE=${SHALE_CALDAV_ENV_COMPOSE:-docker-compose}
        SHALE_MAIL_ENV_COMPOSE=${SHALE_MAIL_ENV_COMPOSE:-docker-compose}
        SHALE_MAIL_ENV_ENGINE=docker
        export SHALE_CALDAV_ENV_COMPOSE SHALE_MAIL_ENV_COMPOSE SHALE_MAIL_ENV_ENGINE
        ;;
      *) echo "no container engine is usable here" >&2; exit 1 ;;
    esac
    echo "Using $engine."

    down() {
      sh tests/caldav-env/caldav-env.sh reset >/dev/null 2>&1 || true
      sh tests/mail-env/mail-env.sh reset >/dev/null 2>&1 || true
    }
    trap down EXIT
    # a failed run on a reused host may have left containers behind
    down

    status=0
    sh tests/caldav-env/caldav-env.sh up || status=1
    sh tests/mail-env/mail-env.sh up || status=1
    if [ "$status" -ne 0 ]; then
      sh tests/caldav-env/caldav-env.sh status || true
      sh tests/mail-env/mail-env.sh status || true
      exit 1
    fi

    for suite in caldav_env mail_env; do
      log=$(mktemp)
      cargo test --locked -- --ignored "$suite" --test-threads=1 2>&1 | tee "$log" || status=1
      # a filter that matches nothing "passes": make sure the suite really ran
      if ! grep -Eq '^test result: ok\. [1-9][0-9]* passed' "$log"; then
        echo "no $suite test passed" >&2
        status=1
      fi
      rm -f "$log"
    done
    exit "$status"
    ;;

  *)
    echo "usage: $0 wanted <event> <base> <head> | detect | run [podman|docker]" >&2
    exit 2
    ;;
esac
