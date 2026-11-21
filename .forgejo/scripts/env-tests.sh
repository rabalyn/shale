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
#       repository, minutes, `tex_install`); the weekly run and release tags of the CI job set it too, not check tags (ADR-0021)
#   env-tests.sh import-wanted <base> <head>
#       prints "yes" or "no": whether the change touches the template import from the network
#       (`assets/tex/sets/`, `src/pdf/tex/import*`, `archive*`, `sets*`, `tests/tex-env/`); only then
#       scripts/land.sh sets SHALE_ENV_TESTS_TEX_IMPORT=yes, which makes `run` also download the
#       pinned tuda-ci from GitHub and the newest from CTAN (`tex_import`, a few MB, seconds); only the
#       weekly run and release tags of the CI job set it too, not check tags (ADR-0021)
#   env-tests.sh detect
#       prints "podman", "docker" or "none": which container engine the job container can use
#   env-tests.sh run [podman|docker]
#       uses the given engine (the one the detect step found; detected again when left out),
#       starts both stacks, builds the TeX image, runs `cargo test -- --ignored calendar::env_tests`,
#       `… mail_env` and `… tex_env` (SHALE_TEX_ENV_REPOSITORY and SHALE_TEX_INSTALL_REPOSITORY, when set (the CI job sets them for v* tags from vars.SHALE_TEX_MIRROR, DEV-025), reach the TeX build and the tex_install test through the environment; the TeX image is built with --refresh when SHALE_ENV_TESTS_TEX_REFRESH=yes, as the weekly run and release tags of the CI job do, not check tags, ADR-0021, so a change of the current TeX Live shows up; `… tex_install` with SHALE_ENV_TESTS_TEX_INSTALL=yes and `… tex_import` with SHALE_ENV_TESTS_TEX_IMPORT=yes), and always takes the stacks down again; exits 1 if a stack or a test fails or no test ran
#       The suites run with one test thread (CalDAV, mail, `tex_install` and `tex_import` share one stack or
#       one install); `tex_env` shares nothing and runs with ${SHALE_ENV_TESTS_TEX_THREADS:-4} test threads (a
#       positive integer, else `run` fails at once with a message; DEV-011, #370)
#       `run` holds the machine-wide lock of the stacks for its whole run (tests/env-lock.sh, DEV-034, #420): a second
#       checkout that wants the stacks says "waiting for the podman test stacks…" and waits (no timeout)
#       `run` sets SHALE_TEX_FAILED_LOGS (default target/tex-failed-logs, emptied first): the TeX suites keep the full
#       log of every failed compile there, and `run` lists them at the end (#327)
#   env-tests.sh tex-suite [threads]
#       the `tex_env` suite as `run` runs it: when it fails with a font that could not be loaded (a mirror or
#       image problem, not a code error, #327), it says so, prints `tex-env.sh status`, builds the image again with
#       --refresh (a fresh mirror) and runs the suite once more; that second run decides. Any other failure is not
#       retried. SHALE_TEX_ENV_SH is the script to use (default tests/tex-env/tex-env.sh; the script tests set it)
set -eu
cd "$(dirname "$0")/../.."
# one checkout at a time uses the podman stacks (DEV-034, #420): `run` and `tex-suite` hold the machine-wide lock
. tests/env-lock.sh

# the cargo output of a suite shows that tests ran and passed (a filter that matches nothing "passes")
passed() { grep -Eq '^test result: ok\. [1-9][0-9]* passed' "$1"; }
# what TeX and luaotfload print when a font cannot be loaded: the image or its mirror, not the code (#327)
font_signature='not loadable|metric data not found'

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

  tex-suite)
    shale_env_lock
    threads=${2:-4}
    texenv=${SHALE_TEX_ENV_SH:-tests/tex-env/tex-env.sh}
    log=$(mktemp)
    trap 'rm -f "$log"' EXIT
    cargo test --locked -- --ignored tex_env --test-threads="$threads" 2>&1 | tee "$log" || true
    if passed "$log"; then exit 0; fi
    if grep -Eiq "$font_signature" "$log"; then
      echo "The tex_env suite failed with a font that could not be loaded: this looks like a TeX environment problem, not a code error" >&2
      echo "(the TeX Live of the image, or the mirror it came from). The image as it is:" >&2
      sh "$texenv" status || true
      echo "Building the TeX image again from a fresh mirror, then running tex_env once more." >&2
      if sh "$texenv" build --refresh; then
        cargo test --locked -- --ignored tex_env --test-threads="$threads" 2>&1 | tee "$log" || true
        if passed "$log"; then exit 0; fi
        echo "tex_env failed again on the fresh image." >&2
      else
        echo "the TeX image could not be built again" >&2
      fi
    fi
    echo "no tex_env test passed" >&2
    exit 1
    ;;

  run)
    # checked first: a typo must not cost the start of the stacks
    tex_threads=${SHALE_ENV_TESTS_TEX_THREADS:-4}
    case $tex_threads in
      ''|*[!0-9]*|0|0[0-9]*) echo "SHALE_ENV_TESTS_TEX_THREADS must be a positive integer, not '$tex_threads'" >&2; exit 1 ;;
    esac

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
    # held for the whole run, children (the helpers, cargo and its tests) see it by SHALE_ENV_LOCK_TOKEN
    shale_env_lock

    # the TeX suites keep the log of every failed compile here (#327): a failure that does not come again can still be read
    failed_logs=${SHALE_TEX_FAILED_LOGS:-target/tex-failed-logs}
    case $failed_logs in /*) ;; *) failed_logs=$PWD/$failed_logs ;; esac
    rm -rf "$failed_logs"
    mkdir -p "$failed_logs"
    SHALE_TEX_FAILED_LOGS=$failed_logs
    export SHALE_TEX_FAILED_LOGS

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
    # land.sh and a check tag use the cached image (CI keeps podman's storage, ADR-0022), the weekly run and release tags rebuild it without the cache
    refresh=""
    if [ "${SHALE_ENV_TESTS_TEX_REFRESH:-no}" = yes ]; then refresh="--refresh"; fi
    sh tests/tex-env/tex-env.sh build $refresh || { echo "the TeX image could not be built" >&2; exit 1; }

    suites="calendar::env_tests mail_env tex_env"
    # the real install of TeX Live downloads about 100 MB and takes minutes: only when asked for
    if [ "${SHALE_ENV_TESTS_TEX_INSTALL:-no}" = yes ]; then suites="$suites tex_install"; fi
    # the real downloads of the template sets (GitHub, CTAN) need the network: only when asked for
    if [ "${SHALE_ENV_TESTS_TEX_IMPORT:-no}" = yes ]; then suites="$suites tex_import"; fi
    for suite in $suites; do
      if [ "$suite" = tex_env ]; then
        # shares nothing, so it runs with several threads; a font problem of the image is tried once more on a fresh mirror
        sh "$0" tex-suite "$tex_threads" || status=1
        continue
      fi
      log=$(mktemp)
      # one stack or one install per suite: one thread
      cargo test --locked -- --ignored "$suite" --test-threads=1 2>&1 | tee "$log" || status=1
      # a filter that matches nothing "passes": make sure the suite really ran
      if ! passed "$log"; then
        echo "no $suite test passed" >&2
        status=1
      fi
      rm -f "$log"
    done
    kept=$(ls "$failed_logs" | wc -l)
    if [ "$kept" -gt 0 ]; then
      echo "The TeX logs of $kept failed compiles are kept in $failed_logs:"
      ls -l "$failed_logs"
    fi
    exit "$status"
    ;;

  *)
    echo "usage: $0 wanted <base> <head> | install-wanted <base> <head> | import-wanted <base> <head> | detect | tex-suite [threads] | run [podman|docker]" >&2
    exit 2
    ;;
esac
