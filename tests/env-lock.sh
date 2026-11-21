# shellcheck shell=sh
# Sourced (`. …/env-lock.sh`, POSIX sh) by everything that uses or changes the podman test stacks
# (DEV-034, #420): .forgejo/scripts/env-tests.sh, tests/{mail,caldav,tex}-env/*.sh. The Rust env
# suites take the same lock (src/env_lock.rs). The stacks have fixed names per machine, so two
# checkouts (worktrees, or a worktree and scripts/land.sh) must not use them at the same time.
#
# The lock is ONE file per user and machine, ${XDG_RUNTIME_DIR:-/tmp}/shale-env-stacks-<user>.lock:
# the runtime dir is per user, tmpfs and cleaned at logout, so a stale file cannot outlive the
# session (the /tmp fallback has the user's name in it, as /tmp is shared); it is outside every
# checkout, so all checkouts and land.sh's worktree meet at it. SHALE_ENV_LOCK_FILE overrides it (tests).
#
# shale_env_lock       waits (no timeout) until the lock is free and takes it for the life of the
#                      calling shell (fd 7, closed again by the OS when the shell ends); says once
#                      that it waits. When the caller is run by a holder it does not wait for it.
#
# Nesting: the holder writes a random token into the lock file and exports it as
# SHALE_ENV_LOCK_TOKEN. A child trusts it only when the file still holds exactly that token AND the
# lock is held right now (a try-lock on a fresh open fails). So a stale value (the holder ended:
# the lock is free) or an inherited one from an older run (another holder wrote another token) never
# skips the lock, and a grandchild (a test process under cargo under env-tests.sh) is covered
# without a parent-PID chain. The token is the same protocol src/env_lock.rs speaks.
# Without `flock` (macOS, a minimal image) the stacks are used without the lock, with a note.
shale_env_lock_file() {
  printf '%s\n' "${SHALE_ENV_LOCK_FILE:-${XDG_RUNTIME_DIR:-/tmp}/shale-env-stacks-${USER:-${LOGNAME:-user}}.lock}"
}

shale_env_lock() {
  _lock=$(shale_env_lock_file)
  if ! command -v flock >/dev/null 2>&1; then
    echo "(flock is not installed: the podman test stacks are used without the machine-wide lock)" >&2
    return 0
  fi
  if [ -n "${SHALE_ENV_LOCK_TOKEN:-}" ] && [ -f "$_lock" ] && [ "$(cat "$_lock" 2>/dev/null)" = "$SHALE_ENV_LOCK_TOKEN" ] && ! flock -n "$_lock" true 2>/dev/null; then
    return 0
  fi
  # append mode: opening must not wipe the token of the holder we may be waiting for
  exec 7>>"$_lock"
  if ! flock -n 7; then
    echo "waiting for the podman test stacks, used by another checkout…" >&2
    flock 7
  fi
  _token="$$-$(od -An -N8 -tx1 /dev/urandom | tr -d ' \n')"
  printf '%s\n' "$_token" > "$_lock"
  SHALE_ENV_LOCK_TOKEN=$_token
  export SHALE_ENV_LOCK_TOKEN
}
