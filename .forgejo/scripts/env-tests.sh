#!/bin/sh
# The CI job "env-tests" (DEV-005, CALENV-004): runs the real-server suites of the CalDAV and mail
# test environments and the LuaLaTeX suite of the TeX environment (TEX-020). The fourth suite, `forgejo_env`
# (SYNC-066), runs the sync wizard's server path against a real Forgejo (`tests/forgejo-env/`): `suites` names it
# for `src/gitsync/wizard/`, `src/gitsync/credentials.rs`, `src/gitsync/lfs.rs`, `src/site_export/publish` (the publish of a vault, SITE-062), `tests/forgejo-env/` and `tests/askpass`; `run` starts
# the Forgejo stack for it (SHALE_FORGEJO_ENV_SH) and takes it down again. Usage:
#   env-tests.sh suites <base> <head>
#       prints the suites the change from <base> to <head> needs, space separated (empty: none):
#       `smb_fuse_env` for `src/netshare_env_tests.rs`, `src/fsutil.rs`, `src/vault.rs` and `tests/smb-env/` (NET-018),
#       `calendar::env_tests` for `src/calendar/` and `tests/caldav-env/`, `mail_env` for `src/mail*` and
#       `tests/mail-env/`, `tex_env` for `src/pdf/tex`, `assets/tex/` and `tests/tex-env/`; all three when
#       `.forgejo/workflows/ci.yml` or this script changes (all five); scripts/land.sh runs those suites locally
#       (FORGE-011, #447)
#   env-tests.sh wanted <base> <head>
#       prints "yes" or "no": whether `suites` names any suite for the same change
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
#       runs the suites of SHALE_ENV_TESTS_SUITES (space separated, from `suites`; unset: all three, as the
#       tag CI runs them; empty or an unknown name: `run` fails at once) and starts only the stacks they need
#       (CalDAV for `calendar::env_tests`, mail for `mail_env`, the Samba server mounted by rclone over FUSE for `smb_fuse_env`) and builds the TeX image only when a TeX suite
#       (`tex_env`, `tex_install`, `tex_import`) runs: `cargo test -- --ignored calendar::env_tests`,
#       `… mail_env` and `… tex_env` (SHALE_TEX_ENV_REPOSITORY and SHALE_TEX_INSTALL_REPOSITORY, when set (the CI job sets them for v* tags from vars.SHALE_TEX_MIRROR, DEV-025), reach the TeX build and the tex_install test through the environment; the TeX image is built with --refresh when SHALE_ENV_TESTS_TEX_REFRESH=yes, as the weekly run of the CI job does, not tags, ADR-0021, ADR-0026, so a change of the current TeX Live shows up; `… tex_install` with SHALE_ENV_TESTS_TEX_INSTALL=yes and `… tex_import` with SHALE_ENV_TESTS_TEX_IMPORT=yes), and always takes the stacks down again; exits 1 if a stack or a test fails or no test ran
#       the stacks and the TeX image build start at the same time (background jobs, a log each), then the suites run at
#       the same time (a log each, printed whole when the suite ends, not interleaved), so a run takes about as long as
#       its slowest suite (DEV-035, #448); `tex_install` and `tex_import` share one install and run one after the other in
#       one job; `run` exits 1 and names the failed suites (`FAILED suites: …`) when any fails
#       between the stacks and the suites `run` runs one `cargo test --locked --no-run` alone (the warm-up, its log shown
#       only when it fails: `run` exits 1 and no suite starts): in a fresh container it installs the toolchain of
#       rust-toolchain.toml and builds the test binary once, so the suites that follow, at the same time, never install it
#       together (#462); `tex-suite` alone runs one cargo and needs none
#       `smb_fuse_env` (NET-018, #477) needs a FUSE mount, rootless (`tests/smb-env/smb-env.sh fuse-mount`: rclone, `/dev/fuse` and
#       `fusermount3`): where the machine has no usable `/dev/fuse` (SHALE_FUSE_DEVICE names it) or no `fusermount3`, `run` says
#       "SKIPPED: smb_fuse_env …" and leaves the suite out (the Forgejo job container has neither: it gets no `--device /dev/fuse`,
#       and the runner's own configuration decides what a job container may take), so the tag CI skips it and scripts/land.sh runs it
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
    if [ -n "$(sh "$0" suites "$2" "$3")" ]; then echo yes; else echo no; fi
    ;;

  suites)
    files=$(git diff --name-only "$2" "$3")
    if printf '%s\n' "$files" | grep -Eq '^\.forgejo/(workflows/ci\.yml|scripts/env-tests\.sh)$'; then
      echo "calendar::env_tests mail_env tex_env forgejo_env smb_fuse_env"
      exit 0
    fi
    out=""
    if printf '%s\n' "$files" | grep -Eq '^(src/calendar/|tests/caldav-env/)'; then out="$out calendar::env_tests"; fi
    if printf '%s\n' "$files" | grep -Eq '^(src/mail|tests/mail-env/)'; then out="$out mail_env"; fi
    if printf '%s\n' "$files" | grep -Eq '^(src/pdf/tex|assets/tex/|tests/tex-env/)'; then out="$out tex_env"; fi
    if printf '%s\n' "$files" | grep -Eq '^(src/gitsync/wizard/|src/gitsync/credentials\.rs|src/gitsync/lfs\.rs|src/site_export/publish|tests/forgejo-env/|tests/askpass)'; then out="$out forgejo_env"; fi
    if printf '%s\n' "$files" | grep -Eq '^(src/netshare_env_tests\.rs|src/fsutil\.rs|src/vault\.rs|tests/smb-env/)'; then out="$out smb_fuse_env"; fi
    echo "${out# }"
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

    # the suites to run: the list of `suites` (land.sh), unset = all (the tag CI)
    suites=${SHALE_ENV_TESTS_SUITES-calendar::env_tests mail_env tex_env forgejo_env smb_fuse_env}
    need_caldav=no; need_mail=no; need_tex=no; need_forgejo=no; need_smb=no
    for suite in $suites; do
      case $suite in
        smb_fuse_env) need_smb=yes ;;
        calendar::env_tests) need_caldav=yes ;;
        mail_env) need_mail=yes ;;
        forgejo_env) need_forgejo=yes ;;
        tex_env) need_tex=yes ;;
        *) echo "SHALE_ENV_TESTS_SUITES names an unknown suite: '$suite'" >&2; exit 1 ;;
      esac
    done
    if [ -z "$suites" ]; then echo "SHALE_ENV_TESTS_SUITES is empty: no suite to run" >&2; exit 1; fi
    # the FUSE mount of the Samba share needs /dev/fuse and fusermount3 (rootless, NET-018): a machine without them skips the suite, loudly
    if [ "$need_smb" = yes ] && { [ ! -c "${SHALE_FUSE_DEVICE:-/dev/fuse}" ] || ! command -v fusermount3 >/dev/null 2>&1; }; then
      echo "SKIPPED: smb_fuse_env: this machine has no usable /dev/fuse and fusermount3 (the job container of the CI gets neither), so the SMB storage checks through rclone did NOT run here (NET-018, tests/smb-env/README.md)."
      need_smb=no
      kept=""
      for suite in $suites; do if [ "$suite" != smb_fuse_env ]; then kept="$kept $suite"; fi; done
      suites=${kept# }
      if [ -z "$suites" ]; then echo "No other suite to run."; exit 0; fi
    fi
    # the real install of TeX Live downloads about 100 MB and takes minutes: only when asked for
    if [ "${SHALE_ENV_TESTS_TEX_INSTALL:-no}" = yes ]; then suites="$suites tex_install"; need_tex=yes; fi
    # the real downloads of the template sets (GitHub, CTAN) need the network: only when asked for
    if [ "${SHALE_ENV_TESTS_TEX_IMPORT:-no}" = yes ]; then suites="$suites tex_import"; need_tex=yes; fi
    caldav_sh=${SHALE_CALDAV_ENV_SH:-tests/caldav-env/caldav-env.sh}
    mail_sh=${SHALE_MAIL_ENV_SH:-tests/mail-env/mail-env.sh}
    forgejo_sh=${SHALE_FORGEJO_ENV_SH:-tests/forgejo-env/forgejo-env.sh}
    smb_sh=${SHALE_SMB_ENV_SH:-tests/smb-env/smb-env.sh}
    texenv=${SHALE_TEX_ENV_SH:-tests/tex-env/tex-env.sh}

    # a second `podman info` right after the first can fail where the first worked: trust the
    # engine the detect step found, and let the stacks themselves show a real problem
    engine=${2:-$(sh "$0" detect)}
    case $engine in
      podman) ;;
      docker)
        SHALE_CALDAV_ENV_COMPOSE=${SHALE_CALDAV_ENV_COMPOSE:-docker-compose}
        SHALE_MAIL_ENV_COMPOSE=${SHALE_MAIL_ENV_COMPOSE:-docker-compose}
        SHALE_FORGEJO_ENV_COMPOSE=${SHALE_FORGEJO_ENV_COMPOSE:-docker-compose}
        SHALE_MAIL_ENV_ENGINE=docker
        SHALE_TEX_ENV_ENGINE=docker
        SHALE_SMB_ENV_ENGINE=docker
        export SHALE_CALDAV_ENV_COMPOSE SHALE_MAIL_ENV_COMPOSE SHALE_FORGEJO_ENV_COMPOSE SHALE_MAIL_ENV_ENGINE SHALE_TEX_ENV_ENGINE SHALE_SMB_ENV_ENGINE
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

    # the parts of the run (the stack starts, the image build, the suites) run as background jobs of this shell (#448):
    # start_job <name> <command…> writes the output to $wd/<name>.log, the seconds to <name>.sec and the exit code
    # to <name>.rc (last, so a log is complete when its .rc is there). A job is a child of the holder of the lock
    # (DEV-034): it sees SHALE_ENV_LOCK_TOKEN and does not wait for it.
    wd=$(mktemp -d)
    job_pids=""
    start_job() {
      _job=$1; shift
      (
        _t0=$(date +%s)
        _rc=0
        "$@" > "$wd/$_job.log" 2>&1 || _rc=$?
        echo $(($(date +%s) - _t0)) > "$wd/$_job.sec"
        echo "$_rc" > "$wd/$_job.rc.tmp"
        mv "$wd/$_job.rc.tmp" "$wd/$_job.rc"
      ) &
      job_pids="$job_pids $!"
    }
    # the polling interval; the self-test (scripts/test-env-tests.sh) shortens it with SHALE_ENV_TESTS_NAP (#672)
    nap() { sleep "${SHALE_ENV_TESTS_NAP:-0.2}" 2>/dev/null || sleep 1; }
    # collect <name>…: waits for the jobs and prints the log of each one when it ended (one after another, never interleaved)
    collect() {
      _pending=$*
      while [ -n "$_pending" ]; do
        _still=""
        for _job in $_pending; do
          if [ -f "$wd/$_job.rc" ]; then
            echo "===== $_job: exit $(cat "$wd/$_job.rc"), $(cat "$wd/$_job.sec") s ====="
            cat "$wd/$_job.log"
          else
            _still="$_still $_job"
          fi
        done
        _pending=${_still# }
        if [ -n "$_pending" ]; then nap; fi
      done
      job_pids=""
    }
    # the jobs of <names> that did not end with 0
    failed_of() {
      for _job in "$@"; do
        if [ "$(cat "$wd/$_job.rc")" != 0 ]; then printf ' %s' "$_job"; fi
      done
    }
    kill_tree() {
      for _child in $(pgrep -P "$1" 2>/dev/null); do kill_tree "$_child"; done
      kill "$1" 2>/dev/null || true
    }

    down() {
      if [ "$need_caldav" = yes ]; then sh "$caldav_sh" reset >/dev/null 2>&1 || true; fi
      if [ "$need_mail" = yes ]; then sh "$mail_sh" reset >/dev/null 2>&1 || true; fi
      if [ "$need_forgejo" = yes ]; then sh "$forgejo_sh" reset >/dev/null 2>&1 || true; fi
      if [ "$need_smb" = yes ]; then sh "$smb_sh" reset >/dev/null 2>&1 || true; fi
    }
    cleanup() {
      # an interrupt: a background job ignores SIGINT, so stop the jobs and what they started
      for _pid in $job_pids; do kill_tree "$_pid"; done
      down
      rm -rf "$wd"
    }
    trap cleanup EXIT
    trap 'exit 1' INT TERM
    # a failed run on a reused host may have left containers behind
    down

    # the stacks and the TeX image do not depend on each other: all of them at the same time. The TeX image is no running
    # stack, only built (the current TeX Live, ADR-0020, and the pinned tuda-ci sources); land.sh and a check tag use the
    # cached image (CI keeps podman's storage, ADR-0022), the weekly run and release tags rebuild it without the cache
    refresh=""
    if [ "${SHALE_ENV_TESTS_TEX_REFRESH:-no}" = yes ]; then refresh="--refresh"; fi
    stacks=""
    if [ "$need_caldav" = yes ]; then start_job up-caldav sh "$caldav_sh" up; stacks="$stacks up-caldav"; fi
    if [ "$need_mail" = yes ]; then start_job up-mail sh "$mail_sh" up; stacks="$stacks up-mail"; fi
    if [ "$need_forgejo" = yes ]; then start_job up-forgejo sh "$forgejo_sh" up; stacks="$stacks up-forgejo"; fi
    # the Samba server, then its rootless FUSE mount (rclone): one job, the mount needs the server
    smb_up() { sh "$smb_sh" up && sh "$smb_sh" fuse-mount; }
    if [ "$need_smb" = yes ]; then start_job up-smb smb_up; stacks="$stacks up-smb"; fi
    prepare=$stacks
    if [ "$need_tex" = yes ]; then start_job tex-build sh "$texenv" build $refresh; prepare="$prepare tex-build"; fi
    # shellcheck disable=SC2086 # the names are words
    collect $prepare
    # shellcheck disable=SC2086
    stacks_failed=$(failed_of $stacks)
    if [ -n "$stacks_failed" ]; then
      echo "a stack did not come up:$stacks_failed" >&2
      if [ "$need_caldav" = yes ]; then sh "$caldav_sh" status || true; fi
      if [ "$need_mail" = yes ]; then sh "$mail_sh" status || true; fi
      if [ "$need_forgejo" = yes ]; then sh "$forgejo_sh" status || true; fi
      if [ "$need_smb" = yes ]; then sh "$smb_sh" status || true; fi
      exit 1
    fi
    if [ "$need_tex" = yes ] && [ -n "$(failed_of tex-build)" ]; then
      echo "the TeX image could not be built" >&2
      exit 1
    fi

    # the warm-up (DEV-035, #462): ONE cargo before the suites go at the same time. In a fresh container the toolchain of
    # rust-toolchain.toml is not installed yet, and several cargo calls at once make rustup install it at the same time
    # (they fail renaming the downloaded files); this one installs it and builds the test binary once, so the suites only run it
    echo "Warming up cargo: cargo test --locked --no-run"
    if ! cargo test --locked --no-run > "$wd/warmup.log" 2>&1; then
      cat "$wd/warmup.log"
      echo "the cargo warm-up (cargo test --locked --no-run) failed: no suite was started" >&2
      exit 1
    fi

    # one suite as `cargo test` runs it, with the check that it really ran: a filter that matches nothing "passes"
    run_suite() {
      _slog=$(mktemp "$wd/suite.XXXXXX")
      _src=0
      # one stack or one install per suite: one thread
      cargo test --locked -- --ignored "$1" --test-threads=1 2>&1 | tee "$_slog" || _src=1
      if ! passed "$_slog"; then
        echo "no $1 test passed" >&2
        _src=1
      fi
      rm -f "$_slog"
      return "$_src"
    }
    # suites one after another (tex_install and tex_import share one install of TeX Live)
    run_suites() {
      _all=0
      for _s in "$@"; do run_suite "$_s" || _all=1; done
      return "$_all"
    }

    # the suites at the same time, each with its own log: a landing takes as long as its slowest suite (#448). Each one has
    # its own stack, so none shares anything with another; the load is small (they mostly wait for a server), only the
    # TeX compiles of `tex_env` need CPU, with their own thread count
    names=""
    serial=""
    for suite in $suites; do
      case $suite in
        tex_env)
          # shares nothing, so it runs with several threads; a font problem of the image is tried once more on a fresh mirror
          start_job tex_env sh "$0" tex-suite "$tex_threads"
          names="$names tex_env"
          ;;
        tex_install|tex_import) serial="$serial $suite" ;;
        *)
          start_job "$suite" run_suites "$suite"
          names="$names $suite"
          ;;
      esac
    done
    if [ -n "$serial" ]; then
      # one job, named by its suites joined with +
      serial_name=$(printf '%s' "${serial# }" | tr ' ' '+')
      # shellcheck disable=SC2086 # the names are words
      start_job "$serial_name" run_suites $serial
      names="$names $serial_name"
    fi
    status=0
    # shellcheck disable=SC2086 # the names are words
    collect $names
    # shellcheck disable=SC2086 # the names are words
    suites_failed=$(failed_of $names)
    if [ -n "$suites_failed" ]; then
      echo "FAILED suites:$suites_failed" >&2
      status=1
    fi
    kept=$(ls "$failed_logs" | wc -l)
    if [ "$kept" -gt 0 ]; then
      echo "The TeX logs of $kept failed compiles are kept in $failed_logs:"
      ls -l "$failed_logs"
    fi
    exit "$status"
    ;;

  *)
    echo "usage: $0 suites <base> <head> | wanted <base> <head> | install-wanted <base> <head> | import-wanted <base> <head> | detect | tex-suite [threads] | run [podman|docker]" >&2
    exit 2
    ;;
esac
