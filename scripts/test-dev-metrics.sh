#!/bin/bash
# Tests scripts/dev-metrics.sh (spec FORGE-016, #368) with a throwaway repository, a fake landing
# folder and fake `gh`, `fj`, `podman`, `pgrep` and `ps` in front of PATH. Metric by metric.
#   bash scripts/test-dev-metrics.sh
set -u
SRC="$(cd "$(dirname "$0")" && pwd)/dev-metrics.sh"
TMP=$(mktemp -d)
cleanup() { [ -z "${HOLD:-}" ] || { pkill -P "$HOLD" 2>/dev/null; kill "$HOLD" 2>/dev/null; }; rm -rf "$TMP"; }
trap cleanup EXIT
FAILED=0
N=0
export GIT_CONFIG_GLOBAL="$TMP/gitconfig"
git config --global user.email t@example.com
git config --global user.name test
git config --global commit.gpgsign false
git config --global init.defaultBranch main

ok() { N=$((N + 1)); if [ "$1" = 0 ]; then echo "ok $N - $2"; else echo "not ok $N - $2"; FAILED=1; fi; }
PROM="$TMP/out/shale.prom"
# has <sample line>: the metric file holds exactly this line
has() {
  N=$((N + 1))
  if grep -Fxq -- "$1" "$PROM"; then echo "ok $N - $1"; else echo "not ok $N - $1 (got: $(grep -F -- "${1%%[ {]*}" "$PROM" | head -n 3 | tr '\n' '|'))"; FAILED=1; fi
}
hasnot() {
  N=$((N + 1))
  if grep -Fq -- "$1" "$PROM"; then echo "not ok $N - absent: $1"; FAILED=1; else echo "ok $N - absent: $1"; fi
}

# --- a repository with an origin, tags and a worktree folder ---------------------------------------
git init -q --bare "$TMP/remote.git"
git clone -q "$TMP/remote.git" "$TMP/repo" 2>/dev/null
mkdir -p "$TMP/repo/scripts" "$TMP/repo/.claude/worktrees/one" "$TMP/repo/.claude/worktrees/two"
cp "$SRC" "$TMP/repo/scripts/dev-metrics.sh"
cd "$TMP/repo" || exit 1
git add scripts && GIT_COMMITTER_DATE="1000000000 +0000" git commit -q -m "a (Refs #1)"
GIT_COMMITTER_DATE="1000000000 +0000" git tag -a v0.1.0 -m "Shale 0.1.0"
GIT_COMMITTER_DATE="1100000000 +0000" git commit -q --allow-empty -m "b (Refs #1)"
B=$(git rev-parse HEAD)
GIT_COMMITTER_DATE="1100000000 +0000" git tag -a v0.2.0-rc.1 -m rc
GIT_COMMITTER_DATE="1200000000 +0000" git commit -q --allow-empty -m "c (Refs #1)"
C=$(git rev-parse HEAD)
GIT_COMMITTER_DATE="1200000000 +0000" git tag -a v0.2.0 -m "Shale 0.2.0"
git push -q origin main
git worktree add -q "$TMP/wt" -b side 2>/dev/null

# --- the landing folder ----------------------------------------------------------------------------
LAND="$TMP/land"
mkdir -p "$LAND"
printf '100\t160\tb1\tlanded\t\t1.5\tmerge=5\tcargo test=40\tpush=2\n'            > "$LAND/history.tsv"
printf '200\t230\tb2\tfailed\tcargo test\t2.25\tmerge=4\tcargo test=20\n'         >> "$LAND/history.tsv"
printf '300\t310\tb3\tconflict\t\t0.5\tmerge=9\n'                                  >> "$LAND/history.tsv"
printf '400\t400\tb4\tnothing\t\t0.1\n'                                          >> "$LAND/history.tsv"
printf '500\t700\tb"5\\x\tlanded\t\t\tmerge=6\tcargo test=60\tpush=3\n'           >> "$LAND/history.tsv"
printf '800\t900\tb6\tfailed\tcargo clippy\t7.5\tmerge=3\tcargo clippy=50\n'      >> "$LAND/history.tsv"

# --- fakes ---------------------------------------------------------------------------------------------
FAKE="$TMP/fake"
mkdir -p "$FAKE/bin" "$TMP/out"
cat > "$FAKE/bin/gh" <<'SH'
#!/bin/sh
echo "gh $*" >> "$FAKE/calls"
[ -z "${FAKE_FAIL:-}" ] || [ "$FAKE_FAIL" != gh ] || exit 1
case $1 in
  issue) cat "$FAKE/issues.tsv" ;;
  release) cat "$FAKE/releases.tsv" ;;
esac
SH
cat > "$FAKE/bin/fj" <<'SH'
#!/bin/sh
echo "fj $*" >> "$FAKE/calls"
[ "${FAKE_FAIL:-}" != fj ] || exit 1
page=""
while [ $# -gt 0 ]; do [ "$1" = -p ] && page=$2; shift; done
[ "$page" = 1 ] && cat "$FAKE/tasks.txt"
exit 0
SH
cat > "$FAKE/bin/podman" <<'SH'
#!/bin/sh
printf 'aaa\nbbb\nccc\n'
SH
cat > "$FAKE/bin/pgrep" <<'SH'
#!/bin/sh
if [ "$1" = -x ]; then cat "$FAKE/pgrep.$2" 2>/dev/null; exit 0; fi
# -f <pattern>: the pids of the lines of procs.txt ("pid command line") that match it
awk -v re="$2" '{ pid = $1; sub(/^[0-9]+ /, ""); if ($0 ~ re) print pid }' "$FAKE/procs.txt" 2>/dev/null
exit 0
SH
cat > "$FAKE/bin/ps" <<'SH'
#!/bin/sh
cat "$FAKE/ps.txt"
SH
chmod +x "$FAKE/bin/"*
export FAKE
printf 'rustc\nrustc\nrustc\n' > "$FAKE/pgrep.rustc"
printf 'cargo\n' > "$FAKE/pgrep.cargo"
printf '1\n2\n3\n4\n' > "$FAKE/pgrep.claude"
: > "$FAKE/procs.txt"
: > "$FAKE/ps.txt"
# state<TAB>createdAt<TAB>closedAt<TAB>labels; NOW is 2000-01-02T00:00:00Z-ish below
NOW=946771200 # 2000-01-02T00:00:00Z
printf 'OPEN\t2000-01-01T12:00:00Z\t\tkind/feature,bug\n'       > "$FAKE/issues.tsv"
printf 'OPEN\t1999-12-01T12:00:00Z\t\t\n'                        >> "$FAKE/issues.tsv"
printf 'CLOSED\t1999-12-01T12:00:00Z\t2000-01-01T18:00:00Z\tbug\n' >> "$FAKE/issues.tsv"
printf 'CLOSED\t1999-11-01T12:00:00Z\t1999-11-02T12:00:00Z\t\n' >> "$FAKE/issues.tsv"
printf 'v0.1.0\t1999-12-31T00:00:00Z\n' > "$FAKE/releases.tsv"
S2=${B:0:10}; S3=${C:0:10}
cat > "$FAKE/tasks.txt" <<TASKS
#9 ($S3) failure build 5s (push): Merge x
#8 ($S3) success env-tests 21m25s (push): Merge x
#7 ($S2) success build 1h2m3s (push): rc
#6 ($S2) skipped mirror 0s (push): rc
#5 ($S2) success env-tests 3m (push): rc
#4 (ffffffffff) success build 7s (push): other
TASKS

run() {
  PATH="$FAKE/bin:$PATH" SHALE_METRICS_DIR="$TMP/out" SHALE_METRICS_LAND_DIR="$LAND" SHALE_METRICS_CACHE_DIR="$TMP/cache" \
    SHALE_METRICS_NOW="$1" bash "$TMP/repo/scripts/dev-metrics.sh" 2>&1
}

# --- a first run: all sources are fetched ------------------------------------------------------------
OUT=$(run $NOW); ok $? "the first run succeeds: $OUT"
has "shale_dev_metrics_last_run_timestamp_seconds $NOW"
has 'shale_dev_metrics_source_ok{source="github"} 1'
has 'shale_dev_metrics_source_ok{source="forgejo"} 1'
has 'shale_dev_metrics_source_ok{source="git"} 1'
has 'shale_dev_metrics_source_age_seconds{source="github"} 0'
[ "$(stat -c %a "$PROM")" = 644 ]; rc=$?; ok $rc "the file is world-readable (node_exporter runs as another user)"
leftover=$(find "$TMP/out" -name '.shale.prom.*' | wc -l); [ "$leftover" = 0 ]; rc=$?; ok $rc "no temp file is left behind"

# landing: idle
has 'shale_land_running 0'
has 'shale_land_queue_waiting 0'
hasnot 'shale_land_current_info'
has 'shale_land_total{result="landed"} 2'
has 'shale_land_total{result="failed"} 2'
has 'shale_land_total{result="conflict"} 1'
has 'shale_land_total{result="nothing"} 1'
has 'shale_land_total{result="error"} 0'
has 'shale_land_duration_seconds_sum{result="landed"} 260'
has 'shale_land_duration_seconds_count{result="landed"} 2'
has 'shale_land_duration_seconds_sum{result="failed"} 130'
has 'shale_land_duration_seconds_count{result="nothing"} 1'
has 'shale_land_last_end_timestamp_seconds 900'
has 'shale_land_last_duration_seconds 100'
has 'shale_land_last_info{branch="b6",result="failed",failed_step="cargo clippy"} 1'
has 'shale_land_last_success_timestamp_seconds 700'
has 'shale_land_step_last_duration_seconds{step="merge"} 3'
has 'shale_land_step_last_duration_seconds{step="cargo test"} 60'
has 'shale_land_step_last_duration_seconds{step="push"} 3'
has 'shale_land_step_duration_seconds_sum{step="cargo test"} 120'
has 'shale_land_step_duration_seconds_count{step="cargo test"} 3'
has 'shale_land_step_duration_seconds_sum{step="merge"} 27'
has 'shale_land_failed_step_total{step="cargo test"} 1'
has 'shale_land_failed_step_total{step="cargo clippy"} 1'
has 'shale_land_last_load1 7.5'

# build, machine, agents
has 'shale_build_processes{kind="rustc"} 3'
has 'shale_build_processes{kind="cargo"} 1'
has 'shale_build_processes{kind="podman"} 0'
has 'shale_podman_containers_running 3'
has 'shale_claude_processes 4'
has 'shale_claude_worktrees 2'
has 'shale_git_worktrees 2'

# issues
has 'shale_github_issues{state="open"} 2'
has 'shale_github_issues{state="closed"} 2'
has 'shale_github_issues_open_by_label{label="kind/feature"} 1'
has 'shale_github_issues_open_by_label{label="bug"} 1'
has 'shale_github_issues_open_by_label{label="none"} 1'
has 'shale_github_issues_created_24h 1'
has 'shale_github_issues_closed_24h 1'

# releases and Forgejo
has 'shale_git_tags{kind="release"} 2'
has 'shale_git_tags{kind="rc"} 1'
has 'shale_github_releases 1'
has 'shale_github_release_published_timestamp_seconds{tag="v0.1.0"} 946598400'
has 'shale_release_attempt{tag="v0.1.0",kind="release",forgejo="none",github_release="yes"} 1000000000'
has 'shale_release_attempt{tag="v0.2.0-rc.1",kind="rc",forgejo="success",github_release="no"} 1100000000'
has 'shale_release_attempt{tag="v0.2.0",kind="release",forgejo="failure",github_release="no"} 1200000000'
has 'shale_release_attempts{kind="release",forgejo="none"} 1'
has 'shale_release_attempts{kind="release",forgejo="failure"} 1'
has 'shale_release_attempts{kind="rc",forgejo="success"} 1'
has 'shale_forgejo_job_last_duration_seconds{job="build"} 5'
has 'shale_forgejo_job_last_duration_seconds{job="env-tests"} 1285'
hasnot 'shale_forgejo_job_last_duration_seconds{job="mirror"}'
has 'shale_forgejo_job_last_result{job="build",result="failure"} 1'
has 'shale_forgejo_job_last_result{job="env-tests",result="success"} 1'

# --- the cache: inside the TTL nothing is fetched ----------------------------------------------------------
calls=$(wc -l < "$FAKE/calls")
run $((NOW + 100)) >/dev/null
[ "$(wc -l < "$FAKE/calls")" = "$calls" ]; rc=$?; ok $rc "inside the TTL the network is not asked again"
has 'shale_dev_metrics_source_age_seconds{source="github"} 100'
has "shale_dev_metrics_last_run_timestamp_seconds $((NOW + 100))"
run $((NOW + 301)) >/dev/null
[ "$(wc -l < "$FAKE/calls")" -gt "$calls" ]; rc=$?; ok $rc "after the TTL it is asked again"
has 'shale_dev_metrics_source_age_seconds{source="github"} 0'
calls=$(wc -l < "$FAKE/calls")
SHALE_METRICS_NET_TTL=10 run $((NOW + 320)) >/dev/null
[ "$(wc -l < "$FAKE/calls")" -gt "$calls" ]; rc=$?; ok $rc "SHALE_METRICS_NET_TTL sets the TTL"

# --- a failing source keeps the old data ----------------------------------------------------------------------
FAKE_FAIL=gh run $((NOW + 1000)) >/dev/null; ok $? "a failing gh does not fail the script"
has 'shale_dev_metrics_source_ok{source="github"} 0'
has 'shale_dev_metrics_source_ok{source="forgejo"} 1'
has 'shale_github_issues{state="open"} 2'
has 'shale_github_releases 1'
has 'shale_dev_metrics_source_age_seconds{source="github"} 680'
FAKE_FAIL=fj run $((NOW + 2000)) >/dev/null
has 'shale_dev_metrics_source_ok{source="forgejo"} 0'
has 'shale_forgejo_job_last_duration_seconds{job="env-tests"} 1285'
has 'shale_dev_metrics_source_ok{source="github"} 1'
mv "$TMP/remote.git" "$TMP/remote.gone"
run $((NOW + 3000)) >/dev/null
has 'shale_dev_metrics_source_ok{source="git"} 0'
has 'shale_git_tags{kind="release"} 2'
mv "$TMP/remote.gone" "$TMP/remote.git"

# --- a running landing and its queue ---------------------------------------------------------------------------
printf '1000\tfeature/x"y\tcargo test\t1030\n' > "$LAND/current"
run $((NOW + 4000)) >/dev/null
has 'shale_land_running 0'
rm -f "$LAND/current"
flock "$LAND/lock" sleep 60 >/dev/null 2>&1 &
HOLD=$!
sleep 0.3
# the running land.sh (11), its subshell (12, parent 11), two waiting ones (13, 14), and shells
# of agents that only mention land.sh (zsh wrapper, grep, an editor): those must not count
cat > "$FAKE/procs.txt" <<PROCS
11 /bin/bash scripts/land.sh claude/a
12 /bin/bash scripts/land.sh claude/a
13 bash ./scripts/land.sh --release claude/b
14 /bin/bash /home/x/app/scripts/land.sh claude/c
21 /bin/zsh -c while pgrep -f "^/bin/bash scripts/land.sh" >/dev/null; do sleep 5; done
22 grep scripts/land.sh docs
23 vim scripts/land.sh
PROCS
printf '11 1\n12 11\n13 1\n14 1\n' > "$FAKE/ps.txt"
run $((NOW + 4001)) >/dev/null
has 'shale_land_running 1'
has 'shale_land_queue_waiting 2'
hasnot 'shale_land_current_info'
hasnot 'shale_land_current_started_timestamp_seconds'
# with `current` (FORGE-015)
printf '1000\tfeature/x"y\tcargo test\t1030\n' > "$LAND/current"
run $((NOW + 4002)) >/dev/null
has 'shale_land_running 1'
has 'shale_land_queue_waiting 2'
has 'shale_land_current_info{branch="feature/x\"y",step="cargo test"} 1'
has 'shale_land_current_started_timestamp_seconds 1000'
has 'shale_land_current_step_started_timestamp_seconds 1030'
pkill -P "$HOLD" 2>/dev/null; kill "$HOLD" 2>/dev/null; wait "$HOLD" 2>/dev/null; HOLD=
rm -f "$LAND/current"
run $((NOW + 4003)) >/dev/null
has 'shale_land_running 0'
has 'shale_land_queue_waiting 3'

# --- an empty landing folder ------------------------------------------------------------------------------------------
rm -f "$LAND/history.tsv"
run $((NOW + 4004)) >/dev/null
has 'shale_land_total{result="landed"} 0'
hasnot 'shale_land_last_info'

# --- the output directory is missing ------------------------------------------------------------------------------------------
OUT=$(PATH="$FAKE/bin:$PATH" SHALE_METRICS_DIR="$TMP/nope" SHALE_METRICS_CACHE_DIR="$TMP/cache" bash "$TMP/repo/scripts/dev-metrics.sh" 2>&1); rc=$?
[ "$rc" = 0 ] && printf '%s' "$OUT" | grep -Fq "skipped"; ok $? "a missing directory is skipped with a message and exit 0"
[ ! -e "$TMP/nope" ]; rc=$?; ok $rc "the missing directory is not created"

# --- promtool, when installed ---------------------------------------------------------------------------------------------------
if command -v promtool >/dev/null 2>&1; then
  promtool check metrics < "$PROM" >/dev/null 2>"$TMP/promtool.err"; ok $? "promtool check metrics accepts the file: $(cat "$TMP/promtool.err")"
else
  echo "ok - promtool is not installed, check skipped"
fi

exit "$FAILED"
