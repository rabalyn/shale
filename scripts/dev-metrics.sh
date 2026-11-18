#!/bin/bash
# Writes the development metrics of Shale (landing, build, agents, issues, releases, Forgejo CI)
# as a Prometheus text file for node_exporter's textfile collector (FORGE-016, #368). Run by the
# systemd user timer of tools/dev-metrics/; the metrics are listed in docs/dev-metrics.md.
#
#   scripts/dev-metrics.sh
#
# Environment:
#   SHALE_METRICS_DIR        where shale.prom goes (default /var/lib/node_exporter/textfile);
#                            when it is missing or not writable the script says so and exits 0
#   SHALE_METRICS_LAND_DIR   the landing folder of land.sh (default <git common dir>/land)
#   SHALE_METRICS_CACHE_DIR  cache of the network data (default ~/.cache/shale-dev-metrics)
#   SHALE_METRICS_NET_TTL    seconds between two refreshes of the network data (default 300)
#   SHALE_METRICS_NOW        a fixed unix time (the test)
# `gh`, `fj`, `git`, `podman` and `pgrep` are found through PATH (the test puts fakes first).
set -euo pipefail

NOW=${SHALE_METRICS_NOW:-$(date +%s)}
OUTDIR=${SHALE_METRICS_DIR:-/var/lib/node_exporter/textfile}
TTL=${SHALE_METRICS_NET_TTL:-300}
CACHE=${SHALE_METRICS_CACHE_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}/shale-dev-metrics}
GH_REPO=rabalyn/shale
FJ_HOST=git.home.rabalyn.de
FJ_REPO=raba/shale

if [ ! -d "$OUTDIR" ] || [ ! -w "$OUTDIR" ]; then
  echo "dev-metrics: $OUTDIR is missing or not writable; skipped (see docs/dev-metrics.md)"
  exit 0
fi

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
COMMON=$(git -C "$HERE" rev-parse --path-format=absolute --git-common-dir)
MAIN=$(dirname "$COMMON")
LAND=${SHALE_METRICS_LAND_DIR:-$COMMON/land}
mkdir -p "$CACHE"

OUT=$(mktemp "$OUTDIR/.shale.prom.XXXXXX")
trap 'rm -f "$OUT"' EXIT

# --- network data, cached ---------------------------------------------------------------------
# Every source keeps its last good data; `<source>.ok` says whether the last try worked and
# `<source>.ts` when the data was fetched. A failing try keeps the old data.
mark() { # <source> <ok 0|1>
  echo "$2" > "$CACHE/$1.ok"
  [ "$2" = 1 ] && echo "$NOW" > "$CACHE/$1.ts"
  return 0
}

fetch_github() {
  local iso
  timeout 60 gh issue list -R "$GH_REPO" --state all --limit 3000 --json state,createdAt,closedAt,labels \
    --jq '.[] | [.state, .createdAt, (.closedAt // ""), ([.labels[].name] | join(","))] | @tsv' > "$CACHE/github-issues.new" &&
    timeout 60 gh release list -R "$GH_REPO" --limit 500 --json tagName,publishedAt \
      --jq '.[] | [.tagName, .publishedAt] | @tsv' > "$CACHE/github-releases.raw" || return 1
  : > "$CACHE/github-releases.new"
  while IFS=$'\t' read -r tag iso; do
    [ -n "$tag" ] || continue
    printf '%s\t%s\n' "$tag" "$(date -u -d "$iso" +%s 2>/dev/null || echo 0)" >> "$CACHE/github-releases.new"
  done < "$CACHE/github-releases.raw"
  mv -f "$CACHE/github-issues.new" "$CACHE/github-issues.tsv"
  mv -f "$CACHE/github-releases.new" "$CACHE/github-releases.tsv"
}

fetch_forgejo() {
  local p page
  : > "$CACHE/forgejo.new"
  for p in 1 2 3 4 5; do
    page=$(timeout 60 fj --host "$FJ_HOST" actions tasks -r "$FJ_REPO" -p "$p") || return 1
    [ -n "$page" ] || break
    printf '%s\n' "$page" >> "$CACHE/forgejo.new"
  done
  mv -f "$CACHE/forgejo.new" "$CACHE/forgejo.tsv"
}

fetch_git() { timeout 60 git -C "$MAIN" fetch -q --tags origin; }

refresh() {
  local src
  for src in github forgejo git; do
    if "fetch_$src"; then mark "$src" 1; else mark "$src" 0; fi
  done
  echo "$NOW" > "$CACHE/attempt"
}

exec 7>>"$CACHE/lock"
attempt=$(cat "$CACHE/attempt" 2>/dev/null || echo 0)
if [ $((NOW - attempt)) -ge "$TTL" ] || [ $((NOW - attempt)) -lt 0 ]; then
  # one refresh at a time; a second run uses the cache
  if flock -n 7; then refresh 2>/dev/null; fi
fi

# --- helpers --------------------------------------------------------------------------------
AWKLIB='
function esc(s) { gsub(/\\/, "\\\\", s); gsub(/"/, "\\\"", s); gsub(/\n/, "\\n", s); return s }
function h(name, type, help) { printf "# HELP %s %s\n# TYPE %s %s\n", name, help, name, type }
'
count() { # lines of stdin, as a number
  local n
  n=$(wc -l)
  echo $((n + 0))
}
family() { printf '# HELP %s %s\n# TYPE %s %s\n' "$1" "$3" "$1" "$2"; }

{
  # --- collector --------------------------------------------------------------------------
  family shale_dev_metrics_last_run_timestamp_seconds gauge "Unix time of this write."
  echo "shale_dev_metrics_last_run_timestamp_seconds $NOW"
  family shale_dev_metrics_source_ok gauge "1 when the last fetch of the source worked."
  for src in github forgejo git; do
    printf 'shale_dev_metrics_source_ok{source="%s"} %s\n' "$src" "$(cat "$CACHE/$src.ok" 2>/dev/null || echo 0)"
  done
  family shale_dev_metrics_source_age_seconds gauge "Age of the cached data of the source."
  for src in github forgejo git; do
    if ts=$(cat "$CACHE/$src.ts" 2>/dev/null); then
      printf 'shale_dev_metrics_source_age_seconds{source="%s"} %s\n' "$src" "$((NOW - ts))"
    fi
  done

  # --- landing ------------------------------------------------------------------------------
  running=0
  if [ -e "$LAND/current" ] && [ -d "$LAND" ]; then
    if ! flock -n "$LAND/lock" true 2>/dev/null; then running=1; fi
  fi
  family shale_land_running gauge "1 while a landing holds the lock."
  echo "shale_land_running $running"

  pids=$(pgrep -f 'scripts/land.sh' 2>/dev/null || true)
  top=0
  if [ -n "$pids" ]; then
    list=$(echo "$pids" | tr '\n' ' ')
    # only top-level land.sh processes: the checks run in a subshell of the same script
    top=$(ps -o pid= -o ppid= -p "${list// /,}" 2>/dev/null | awk -v pl=" $list" '
      { pid[NR] = $1; ppid[NR] = $2 }
      END { n = 0; for (i = 1; i <= NR; i++) if (index(pl, " " ppid[i] " ") == 0) n++; print n }' || true)
    top=$((${top:-0} + 0))
  fi
  waiting=$((top - running))
  [ "$waiting" -ge 0 ] || waiting=0
  family shale_land_queue_waiting gauge "land.sh processes waiting for the lock."
  echo "shale_land_queue_waiting $waiting"

  if [ "$running" = 1 ]; then
    IFS=$'\t' read -r cstart cbranch cstep cstepstart < "$LAND/current" || true
    awk -v b="$cbranch" -v s="$cstep" "$AWKLIB"'BEGIN {
      h("shale_land_current_info", "gauge", "The running landing: branch and step.")
      printf "shale_land_current_info{branch=\"%s\",step=\"%s\"} 1\n", esc(b), esc(s) }'
    family shale_land_current_started_timestamp_seconds gauge "Start of the running landing."
    echo "shale_land_current_started_timestamp_seconds ${cstart:-0}"
    family shale_land_current_step_started_timestamp_seconds gauge "Start of the current step."
    echo "shale_land_current_step_started_timestamp_seconds ${cstepstart:-0}"
  fi

  if [ -s "$LAND/history.tsv" ]; then
    awk -F'\t' "$AWKLIB"'
      BEGIN { nres = split("landed failed conflict nothing error", R, " ") }
      NF >= 4 {
        tot[$4]++; sum[$4] += $2 - $1; cnt[$4]++
        lastend = $2; lastdur = $2 - $1; lastb = $3; lastr = $4; lastf = $5; load = $6
        if ($4 == "landed") succ = $2
        if ($4 == "failed" && $5 != "") failed[$5]++
        for (i = 7; i <= NF; i++) {
          if (!match($i, /=[0-9]+$/)) continue
          name = substr($i, 1, RSTART - 1); sec = substr($i, RSTART + 1)
          if (!(name in ssum)) order[++nord] = name
          ssum[name] += sec; scnt[name]++; slast[name] = sec
        }
      }
      END {
        h("shale_land_total", "counter", "Landings by result.")
        for (i = 1; i <= nres; i++) printf "shale_land_total{result=\"%s\"} %d\n", R[i], tot[R[i]]
        h("shale_land_duration_seconds_sum", "counter", "Sum of the landing durations by result.")
        for (i = 1; i <= nres; i++) printf "shale_land_duration_seconds_sum{result=\"%s\"} %d\n", R[i], sum[R[i]]
        h("shale_land_duration_seconds_count", "counter", "Number of landings by result.")
        for (i = 1; i <= nres; i++) printf "shale_land_duration_seconds_count{result=\"%s\"} %d\n", R[i], cnt[R[i]]
        if (lastr != "") {
          h("shale_land_last_end_timestamp_seconds", "gauge", "End of the last finished landing.")
          printf "shale_land_last_end_timestamp_seconds %d\n", lastend
          h("shale_land_last_duration_seconds", "gauge", "Duration of the last finished landing.")
          printf "shale_land_last_duration_seconds %d\n", lastdur
          h("shale_land_last_info", "gauge", "The last finished landing: branch, result, failing step.")
          printf "shale_land_last_info{branch=\"%s\",result=\"%s\",failed_step=\"%s\"} 1\n", esc(lastb), esc(lastr), esc(lastf)
        }
        if (succ != "") {
          h("shale_land_last_success_timestamp_seconds", "gauge", "End of the last landing that landed.")
          printf "shale_land_last_success_timestamp_seconds %d\n", succ
        }
        h("shale_land_step_last_duration_seconds", "gauge", "Last duration of each step.")
        for (i = 1; i <= nord; i++) printf "shale_land_step_last_duration_seconds{step=\"%s\"} %d\n", esc(order[i]), slast[order[i]]
        h("shale_land_step_duration_seconds_sum", "counter", "Sum of the durations of each step.")
        for (i = 1; i <= nord; i++) printf "shale_land_step_duration_seconds_sum{step=\"%s\"} %d\n", esc(order[i]), ssum[order[i]]
        h("shale_land_step_duration_seconds_count", "counter", "Number of runs of each step.")
        for (i = 1; i <= nord; i++) printf "shale_land_step_duration_seconds_count{step=\"%s\"} %d\n", esc(order[i]), scnt[order[i]]
        h("shale_land_failed_step_total", "counter", "Landings that failed at the step.")
        for (s in failed) printf "shale_land_failed_step_total{step=\"%s\"} %d\n", esc(s), failed[s]
        if (load != "") {
          h("shale_land_last_load1", "gauge", "1-minute load at the end of the last landing.")
          printf "shale_land_last_load1 %s\n", load
        }
      }' "$LAND/history.tsv"
  else
    awk "$AWKLIB"'BEGIN {
      split("landed failed conflict nothing error", R, " ")
      h("shale_land_total", "counter", "Landings by result.")
      for (i = 1; i <= 5; i++) printf "shale_land_total{result=\"%s\"} 0\n", R[i]
      h("shale_land_duration_seconds_sum", "counter", "Sum of the landing durations by result.")
      for (i = 1; i <= 5; i++) printf "shale_land_duration_seconds_sum{result=\"%s\"} 0\n", R[i]
      h("shale_land_duration_seconds_count", "counter", "Number of landings by result.")
      for (i = 1; i <= 5; i++) printf "shale_land_duration_seconds_count{result=\"%s\"} 0\n", R[i] }'
  fi

  # --- build and machine ----------------------------------------------------------------------
  family shale_build_processes gauge "Build processes on the machine."
  for kind in rustc cargo podman lualatex; do
    n=$( (pgrep -x "$kind" 2>/dev/null || true) | count)
    printf 'shale_build_processes{kind="%s"} %s\n' "$kind" "$n"
  done
  family shale_podman_containers_running gauge "Running podman containers."
  if command -v podman >/dev/null 2>&1; then
    n=$( (podman ps -q 2>/dev/null || true) | count)
  else
    n=0
  fi
  echo "shale_podman_containers_running $n"

  # --- agents -----------------------------------------------------------------------------------
  family shale_claude_processes gauge "Claude Code processes on the machine."
  echo "shale_claude_processes $( (pgrep -x claude 2>/dev/null || true) | count)"
  family shale_claude_worktrees gauge "Directories under .claude/worktrees/ of the main checkout."
  n=0
  if [ -d "$MAIN/.claude/worktrees" ]; then
    n=$(find "$MAIN/.claude/worktrees" -mindepth 1 -maxdepth 1 -type d | count)
  fi
  echo "shale_claude_worktrees $n"
  family shale_git_worktrees gauge "Entries of git worktree list."
  echo "shale_git_worktrees $( (git -C "$MAIN" worktree list 2>/dev/null || true) | count)"

  # --- issues -----------------------------------------------------------------------------------
  if [ -f "$CACHE/github-issues.tsv" ]; then
    cutoff=$(date -u -d "@$((NOW - 86400))" +%Y-%m-%dT%H:%M:%SZ)
    awk -F'\t' -v cutoff="$cutoff" "$AWKLIB"'
      { st = tolower($1); n[st]++
        if ($1 == "OPEN" || st == "open") {
          if ($4 == "") by["none"]++
          else { k = split($4, L, ","); for (i = 1; i <= k; i++) by[L[i]]++ }
        }
        if ($2 >= cutoff) created++
        if ($3 != "" && $3 >= cutoff) closed++ }
      END {
        h("shale_github_issues", "gauge", "Issues of rabalyn/shale by state.")
        printf "shale_github_issues{state=\"open\"} %d\nshale_github_issues{state=\"closed\"} %d\n", n["open"], n["closed"]
        h("shale_github_issues_open_by_label", "gauge", "Open issues by label (none: no label).")
        for (l in by) printf "shale_github_issues_open_by_label{label=\"%s\"} %d\n", esc(l), by[l]
        h("shale_github_issues_created_24h", "gauge", "Issues created in the last 24 hours.")
        printf "shale_github_issues_created_24h %d\n", created
        h("shale_github_issues_closed_24h", "gauge", "Issues closed in the last 24 hours.")
        printf "shale_github_issues_closed_24h %d\n", closed }' "$CACHE/github-issues.tsv"
  fi

  # --- releases ---------------------------------------------------------------------------------
  tags=$(git -C "$MAIN" tag -l 'v*' --sort=version:refname \
    --format='%(refname:strip=2)%09%(creatordate:unix)%09%(if)%(*objectname)%(then)%(*objectname)%(else)%(objectname)%(end)' 2>/dev/null || true)
  echo "$tags" | awk -F'\t' "$AWKLIB"'
    NF >= 3 { if (index($1, "-")) rc++; else rel++ }
    END {
      h("shale_git_tags", "gauge", "Tags v* by kind (rc: pre-release).")
      printf "shale_git_tags{kind=\"release\"} %d\nshale_git_tags{kind=\"rc\"} %d\n", rel, rc }'
  if [ -f "$CACHE/github-releases.tsv" ]; then
    awk -F'\t' "$AWKLIB"'
      { n++; t[n] = $1; p[n] = $2 }
      END {
        h("shale_github_releases", "gauge", "GitHub releases.")
        printf "shale_github_releases %d\n", n
        h("shale_github_release_published_timestamp_seconds", "gauge", "Publication time of each GitHub release.")
        for (i = 1; i <= n; i++) printf "shale_github_release_published_timestamp_seconds{tag=\"%s\"} %d\n", esc(t[i]), p[i] }' "$CACHE/github-releases.tsv"
  fi

  # one run of Forgejo per tag: the tasks whose commit is the tag'"'"'s commit
  rel_file=/dev/null
  [ -f "$CACHE/github-releases.tsv" ] && rel_file="$CACHE/github-releases.tsv"
  fj_file=/dev/null
  [ -f "$CACHE/forgejo.tsv" ] && fj_file="$CACHE/forgejo.tsv"
  { [ -z "$tags" ] || printf '%s\n' "$tags" | sed 's/^/T\t/'; sed 's/^/R\t/' "$rel_file"; sed 's/^/F\t/' "$fj_file"; } |
    awk -F'\t' "$AWKLIB"'
      function secs(d,   hh, mm, ss) {
        hh = mm = ss = 0
        if (match(d, /[0-9]+h/)) hh = substr(d, RSTART, RLENGTH - 1)
        if (match(d, /[0-9]+m/)) mm = substr(d, RSTART, RLENGTH - 1)
        if (match(d, /[0-9]+s/)) ss = substr(d, RSTART, RLENGTH - 1)
        return hh * 3600 + mm * 60 + ss
      }
      $1 == "T" && NF >= 4 { nt++; tag[nt] = $2; tt[nt] = $3; sha[nt] = $4 }
      $1 == "R" { gh[$2] = 1 }
      $1 == "F" {
        line = $2
        for (i = 3; i <= NF; i++) line = line "\t" $i
        if (match(line, /^#[0-9]+ \([0-9a-f]+\) /) == 0) next
        split(line, w, " ")
        id = substr(w[1], 2) + 0; s = w[2]; gsub(/[()]/, "", s)
        st = w[3]; job = w[4]; dur = secs(w[5])
        fn++; fid[fn] = id; fsha[fn] = s; fst[fn] = st
        if (st == "success" || st == "failure" || st == "cancelled") {
          if (!(job in jid) || id > jid[job]) { jid[job] = id; jdur[job] = dur; jres[job] = st }
        }
      }
      END {
        h("shale_release_attempt", "gauge", "One per v* tag: Forgejo result and GitHub release; the value is the time of the tag.")
        for (i = 1; i <= nt; i++) {
          kind = index(tag[i], "-") ? "rc" : "release"
          bad = 0; run = 0; ok = 0
          for (j = 1; j <= fn; j++) {
            if (sha[i] == "" || substr(sha[i], 1, length(fsha[j])) != fsha[j]) continue
            if (fst[j] == "failure" || fst[j] == "cancelled") bad = 1
            else if (fst[j] == "running" || fst[j] == "waiting") run = 1
            else ok = 1
          }
          res = bad ? "failure" : run ? "running" : ok ? "success" : "none"
          printf "shale_release_attempt{tag=\"%s\",kind=\"%s\",forgejo=\"%s\",github_release=\"%s\"} %d\n", esc(tag[i]), kind, res, (tag[i] in gh) ? "yes" : "no", tt[i]
          cnt[kind SUBSEP res]++
        }
        h("shale_release_attempts", "gauge", "Tags by kind and Forgejo result.")
        for (k in cnt) { split(k, p, SUBSEP); printf "shale_release_attempts{kind=\"%s\",forgejo=\"%s\"} %d\n", p[1], p[2], cnt[k] }
        h("shale_forgejo_job_last_duration_seconds", "gauge", "Duration of the last finished Forgejo run of each job.")
        for (j in jid) printf "shale_forgejo_job_last_duration_seconds{job=\"%s\"} %d\n", esc(j), jdur[j]
        h("shale_forgejo_job_last_result", "gauge", "Result of the last finished Forgejo run of each job.")
        for (j in jid) printf "shale_forgejo_job_last_result{job=\"%s\",result=\"%s\"} 1\n", esc(j), jres[j]
      }'
} > "$OUT"

chmod 0644 "$OUT"
mv -f "$OUT" "$OUTDIR/shale.prom"
