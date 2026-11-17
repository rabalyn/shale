#!/bin/sh
# The CI job "env-tests" (DEV-005, CALENV-004): runs the real-server suites of the CalDAV and mail
# test environments and the LuaLaTeX suite of the TeX environment (TEX-020). Usage:
#   env-tests.sh wanted <base> <head>
#       prints "yes" or "no": whether the change from <base> to <head> touches the CalDAV, mail or
#       TeX export code or the environments; scripts/land.sh runs the suites locally then (FORGE-011)
#   env-tests.sh install-wanted <base> <head>
#       prints "yes" or "no": whether the change touches the managed TeX Live install (`assets/tex/`,
#       `src/pdf/tex/install*`); only then scripts/land.sh sets SHALE_ENV_TESTS_TEX_INSTALL=yes, which
#       makes `run` also do the real install of TeX Live (about 100 MB from the default
#       repository, minutes, `tex_install`); the weekly schedule of the CI job sets it too
#   env-tests.sh import-wanted <base> <head>
#       prints "yes" or "no": whether the change touches the template import from the network
#       (`assets/tex/sets/`, `src/pdf/tex/import*`, `archive*`, `sets*`, `tests/tex-env/`); only then
#       scripts/land.sh sets SHALE_ENV_TESTS_TEX_IMPORT=yes, which makes `run` also download the
#       pinned tuda-ci from GitHub and the newest from CTAN (`tex_import`, a few MB, seconds); the
#       CI job sets it too
#   env-tests.sh detect
#       prints "podman", "docker" or "none": which container engine the job container can use
#   env-tests.sh run [podman|docker]
#       uses the given engine (the one the detect step found; detected again when left out),
#       starts both stacks, builds the TeX image, runs `cargo test -- --ignored calendar::env_tests`,
#       `… mail_env` and `… tex_env` (the TeX image is built with --refresh when SHALE_ENV_TESTS_TEX_REFRESH=yes, as the CI job does, so a change of the current TeX Live shows up; `… tex_install` with SHALE_ENV_TESTS_TEX_INSTALL=yes and `… tex_import` with SHALE_ENV_TESTS_TEX_IMPORT=yes), and always takes the stacks down again; exits 1 if a stack or a test fails or no test ran
set -eu
cd "$(dirname "$0")/../.."

case "${1:-}" in
  wanted)
    files=$(git diff --name-only "$2" "$3")
    if printf '%s\n' "$files" | grep -Eq '^(src/(calendar/|mail|pdf/tex)|assets/tex/|tests/(caldav|mail|tex)-env/|\.forgejo/(workflows/ci\.yml|scripts/env-tests\.sh))'; then
      echo yes
    else
      echo no
    fi
    ;;

  install-wanted)
    files=$(git diff --name-only "$2" "$3")
    if printf '%s\n' "$files" | grep -Eq '^(assets/tex/|src/pdf/tex/install|\.forgejo/scripts/env-tests\.sh)'; then
      echo yes
    else
      echo no
    fi
    ;;

  import-wanted)
    files=$(git diff --name-only "$2" "$3")
    if printf '%s\n' "$files" | grep -Eq '^(assets/tex/sets/|src/pdf/tex/(import|archive|sets)|tests/tex-env/|\.forgejo/scripts/env-tests\.sh)'; then
      echo yes
    else
      echo no
    fi
    ;;

  detect)
    # `podman info` alone is not enough: an unprivileged job container answers it but cannot unpack
    # images with files owned by other ids (tests/caldav-env/README.md "In CI"). Alpine's
    # /etc/shadow belongs to group 42, the owner the stack images failed on, so start it.
    probe=docker.io/library/alpine:3.20
    if command -v podman >/dev/null 2>&1 && podman info >/dev/null 2>&1 && podman run --rm "$probe" true >/dev/null 2>&1; then
      echo podman
    elif command -v docker >/dev/null 2>&1 && docker info >/dev/null 2>&1 && docker run --rm "$probe" true >/dev/null 2>&1; then
      echo docker
    else
      echo none
      # say why, for the log (stderr, so the answer on stdout stays one word)
      if command -v podman >/dev/null 2>&1; then podman run --rm "$probe" true 2>&1 | tail -n 3 >&2 || true; fi
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
        SHALE_TEX_ENV_ENGINE=docker
        export SHALE_CALDAV_ENV_COMPOSE SHALE_MAIL_ENV_COMPOSE SHALE_MAIL_ENV_ENGINE SHALE_TEX_ENV_ENGINE
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

    # the TeX image is no running stack: only built (the current TeX Live, ADR-0020, and the pinned tuda-ci sources);
    # land.sh uses the cached image, CI rebuilds it without the cache
    refresh=""
    if [ "${SHALE_ENV_TESTS_TEX_REFRESH:-no}" = yes ]; then refresh="--refresh"; fi
    sh tests/tex-env/tex-env.sh build $refresh || { echo "the TeX image could not be built" >&2; exit 1; }

    suites="calendar::env_tests mail_env tex_env"
    # the real install of TeX Live downloads about 100 MB and takes minutes: only when asked for
    if [ "${SHALE_ENV_TESTS_TEX_INSTALL:-no}" = yes ]; then suites="$suites tex_install"; fi
    # the real downloads of the template sets (GitHub, CTAN) need the network: only when asked for
    if [ "${SHALE_ENV_TESTS_TEX_IMPORT:-no}" = yes ]; then suites="$suites tex_import"; fi
    for suite in $suites; do
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
    echo "usage: $0 wanted <base> <head> | install-wanted <base> <head> | import-wanted <base> <head> | detect | run [podman|docker]" >&2
    exit 2
    ;;
esac
