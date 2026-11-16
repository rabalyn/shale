#!/bin/sh
# Reports a red run of the GitHub workflow (the `macos` and `windows` jobs, run for every push to
# `main`, BUILD-017) back to Forgejo as an issue (BUILD-020, raba/shale#94). The scheduled Forgejo
# workflow `github-report` runs it; it can be run by hand too. Needs `curl` and `jq`.
#
# Environment:
#   GITHUB_API       default https://api.github.com
#   GITHUB_REPO      default rabalyn/shale (public: reading needs no token)
#   GITHUB_WORKFLOW  default build.yml
#   FORGEJO_API      required, e.g. https://git.home.rabalyn.de/api/v1
#   FORGEJO_REPO     required, e.g. raba/shale
#   FORGEJO_TOKEN    required, a token that may write issues of FORGEJO_REPO
#
# Rules:
# - The newest 20 push runs of `main` are handled oldest first. A run that is not completed stops
#   the pass (a later run is never reported before it); a cancelled, skipped or neutral run is ignored.
# - A red run (any other conclusion than success) opens the issue "GitHub CI red on main: ...", or,
#   while the newest such issue is open, is added to it as a comment. The first green run after that
#   adds a comment and closes the issue; a later red run opens a new one.
# - Nothing is reported twice: the issue and each comment carry the marker
#   <!-- shale-github-run:RUN-ID:failure|success -->, and the marker with the highest run id is
#   the watermark. A run is looked at again only when it was re-run on GitHub with another result.
# - With no such issue yet only the current red streak (the runs after the newest green one) is
#   reported, not the history.
# - Any problem (GitHub or Forgejo unreachable or refusing, a missing setting) ends with an
#   `::error::` line and exit status 1. The token is never printed.

set -eu

PREFIX='GitHub CI red on main'
PREFIX_QUERY='GitHub%20CI%20red%20on%20main'

die() {
  echo "::error::$*"
  exit 1
}

GITHUB_API=${GITHUB_API:-https://api.github.com}
GITHUB_REPO=${GITHUB_REPO:-rabalyn/shale}
GITHUB_WORKFLOW=${GITHUB_WORKFLOW:-build.yml}
[ -n "${FORGEJO_API:-}" ] || die "FORGEJO_API is not set (the address of Forgejo's API, e.g. https://git.home.rabalyn.de/api/v1)"
[ -n "${FORGEJO_REPO:-}" ] || die "FORGEJO_REPO is not set (owner/name, e.g. raba/shale)"
[ -n "${FORGEJO_TOKEN:-}" ] || die "FORGEJO_TOKEN is not set or empty (see the secret ISSUE_REPORT_TOKEN in .forgejo/workflows/github-report.yml)"
command -v curl >/dev/null 2>&1 || die "curl is not installed (needed to talk to GitHub and Forgejo)"
command -v jq >/dev/null 2>&1 || die "jq is not installed (needed to read the answers; apt-get install jq)"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

# curl with timeouts and no retries; the body goes to $tmp/body, the HTTP status is printed
http() {
  curl -sS --connect-timeout 10 --max-time 30 -o "$tmp/body" -w '%{http_code}' "$@"
}

# gh_get PATH KEY: the answer's JSON is in $tmp/body and has a list under KEY
gh_get() {
  code=$(http -H 'Accept: application/vnd.github+json' -H 'X-GitHub-Api-Version: 2022-11-28' \
    "$GITHUB_API$1") || die "GitHub is not reachable: GET $GITHUB_API$1"
  case $code in
    200) ;;
    403 | 429) die "GitHub answered $code for GET $1 (probably the rate limit for requests without a token; try again later)" ;;
    *) die "GitHub answered $code for GET $1" ;;
  esac
  jq -e --arg k "$2" '.[$k] | type == "array"' "$tmp/body" >/dev/null 2>&1 \
    || die "GitHub's answer for GET $1 is not the expected JSON (no list \"$2\")"
}

# fj_request METHOD PATH [JSON-BODY]: the answer's JSON is in $tmp/body
fj_request() {
  url="$FORGEJO_API/repos/$FORGEJO_REPO$2"
  if [ $# -ge 3 ]; then
    code=$(http -X "$1" -H "Authorization: token $FORGEJO_TOKEN" -H 'Content-Type: application/json' \
      --data "$3" "$url") || die "Forgejo is not reachable: $1 $url"
  else
    code=$(http -X "$1" -H "Authorization: token $FORGEJO_TOKEN" "$url") \
      || die "Forgejo is not reachable: $1 $url"
  fi
  case $code in
    2??) ;;
    401 | 403) die "Forgejo answered $code for $1 $2: the token may not write issues of $FORGEJO_REPO. The owner has to create a Forgejo access token with write access to issues for this repository and store it as the Actions secret ISSUE_REPORT_TOKEN (names starting with GITHUB_ are refused)." ;;
    *) die "Forgejo answered $code for $1 $2" ;;
  esac
  jq -e . "$tmp/body" >/dev/null 2>&1 || die "Forgejo's answer for $1 $2 is not JSON"
}

say() {
  echo "$*"
  did=1
}

# the text that says what happened in a run: commit, links, failed jobs, the marker
detail() { # RUN-JSON FAILED-JOBS-JSON RESULT
  jq -rn --argjson run "$1" --argjson jobs "$2" --arg result "$3" '
    "Commit `\($run.sha)`: \($run.msg)\n\nRun: \($run.url)\n\n"
    + (if ($jobs | length) == 0 then "No failed job is listed for this run.\n"
       else "Failed jobs:\n" + ($jobs | map("- \(.name): \(.conclusion) (\(.url))\n") | join("")) end)
    + "\n<!-- shale-github-run:\($run.id):\($result) -->"'
}

# sets floor and wm_result from the markers in the body and all comments of issue $issue_n
read_watermark() {
  fj_request GET "/issues/$issue_n"
  jq -r '.body' "$tmp/body" >"$tmp/text"
  page=1
  while :; do
    fj_request GET "/issues/$issue_n/comments?page=$page&limit=50"
    [ "$(jq length "$tmp/body")" -gt 0 ] || break
    jq -r '.[].body' "$tmp/body" >>"$tmp/text"
    page=$((page + 1))
  done
  marker=$(jq -Rrs '[scan("<!-- shale-github-run:([0-9]+):(failure|success) -->") | {id: (.[0] | tonumber), result: .[1]}]
    | max_by(.id) // empty | "\(.id) \(.result)"' "$tmp/text")
  if [ -n "$marker" ]; then
    floor=${marker% *}
    wm_result=${marker#* }
  fi
}

comment() { # TEXT
  fj_request POST "/issues/$issue_n/comments" "$(jq -n --arg body "$1" '{body: $body}')"
}

on_red() { # RUN-JSON
  gh_get "/repos/$GITHUB_REPO/actions/runs/$id/jobs?per_page=100" jobs
  failed=$(jq -c '[.jobs[] | select(.conclusion != null and .conclusion != "success" and .conclusion != "skipped")
    | {name, conclusion, url: .html_url}]' "$tmp/body")
  text=$(detail "$1" "$failed" failure)
  if [ "$issue_open" = yes ]; then
    comment "$(printf 'Run %s is red too.\n\n%s' "$id" "$text")"
    say "commented on #$issue_n: run $id is red too"
    return
  fi
  title=$(jq -rn --argjson run "$1" --argjson jobs "$failed" --arg prefix "$PREFIX" '
    "\($prefix): \(if ($jobs | length) == 0 then "run" else ($jobs | map(.name) | join(", ")) end) failed at \($run.sha[:7])"')
  body=$(printf "The macOS or Windows run of \`main\` on GitHub is red.\n\n%s\n\nFurther red runs are added to this issue as comments, and it is closed when a run of \`main\` is green again. Made by \`scripts/report-github-ci.sh\` (BUILD-020)." "$text")
  fj_request POST /issues "$(jq -n --arg title "$title" --arg body "$body" '{title: $title, body: $body}')"
  issue_n=$(jq -r '.number' "$tmp/body")
  issue_open=yes
  say "opened issue #$issue_n for run $id: $title"
}

on_green() { # RUN-JSON
  [ "$issue_open" = yes ] || return 0
  comment "$(printf "\`main\` is green again.\n\n%s" "$(detail "$1" '[]' success)")"
  fj_request PATCH "/issues/$issue_n" '{"state":"closed"}'
  issue_open=no
  say "closed #$issue_n: run $id is green"
}

# --- 1. the runs of GitHub, oldest first
gh_get "/repos/$GITHUB_REPO/actions/workflows/$GITHUB_WORKFLOW/runs?branch=main&event=push&per_page=20" workflow_runs
jq -c '.workflow_runs | sort_by(.id) | .[]
  | {id, sha: .head_sha, status, conclusion, url: .html_url, msg: ((.head_commit.message // "") | split("\n")[0] // "")}' \
  "$tmp/body" >"$tmp/runs"
if [ ! -s "$tmp/runs" ]; then
  echo "GitHub lists no run of $GITHUB_WORKFLOW for main: nothing to report"
  exit 0
fi

# --- 2. the newest report issue and the watermark: the runs up to $floor are done
fj_request GET "/issues?state=all&type=issues&q=$PREFIX_QUERY&limit=50"
newest=$(jq -r --arg prefix "$PREFIX" '[.[] | select(.title | startswith($prefix))]
  | max_by(.number) // empty | "\(.number) \(.state)"' "$tmp/body")
issue_n=
issue_open=no
floor=0
wm_result=
if [ -n "$newest" ]; then
  issue_n=${newest% *}
  [ "${newest#* }" != open ] || issue_open=yes
  read_watermark
else
  # first use: only the current red streak, the runs after the newest green one
  floor=$(jq -s '[.[] | select(.conclusion == "success") | .id] | max // 0' "$tmp/runs")
fi

# --- 3. the runs after the watermark, and the watermark's own when a re-run changed its result
did=0
while IFS= read -r run; do
  fields=$(printf '%s' "$run" | jq -r '[.id,
    (if .status != "completed" or .conclusion == null then "pending"
     elif .conclusion == "success" then "green"
     elif (.conclusion | IN("cancelled", "skipped", "neutral")) then "ignored"
     else "red" end)] | join(" ")')
  id=${fields% *}
  kind=${fields#* }
  [ "$id" -ge "$floor" ] || continue
  if [ "$id" -eq "$floor" ]; then
    case $kind:$wm_result in
      green:failure | red:success) ;;
      *) continue ;;
    esac
  fi
  case $kind in
    pending)
      say "run $id is still running: later runs wait for it"
      break
      ;;
    ignored) ;;
    green) on_green "$run" ;;
    red) on_red "$run" ;;
  esac
done <"$tmp/runs"

[ "$did" -eq 1 ] || echo "nothing new"
