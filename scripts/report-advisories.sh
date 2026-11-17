#!/bin/sh
# Turns the findings of `cargo audit` into GitHub issues (BUILD-021, #257), so an agent can pick
# them up like any other work. The `audit` job of .forgejo/workflows/ci.yml runs it after
# `cargo audit --json`, daily and for every push to `main`; it can be run by hand too.
# Needs `curl` and `jq`.
#
#   sh scripts/report-advisories.sh AUDIT-JSON
#
# Environment:
#   GH_TOKEN      required, a GitHub token that may write the issues of GITHUB_REPO
#   GITHUB_API    default https://api.github.com
#   GITHUB_REPO   default rabalyn/shale
#   AUDIT_COMMIT  optional, the commit that was audited (named in the texts)
#
# Rules:
# - One finding is one advisory (its RUSTSEC id) or one yanked crate version; the crate versions
#   it hits are listed together. Vulnerabilities and the warnings unmaintained, unsound and yanked
#   are all reported, with the label `security` and one of `security/vulnerability`,
#   `security/unmaintained`, `security/unsound`, `security/yanked` (made when missing).
# - The issue carries the marker <!-- shale-advisory:KEY -->; only issues labelled `security`
#   with such a marker are the script's, others are never touched.
# - A finding with no issue opens one. With an open issue nothing happens.
# - An open issue whose finding is gone gets a comment and is closed as completed.
# - A finding whose issue was closed as completed (by this script or by a fix) is reported again:
#   the issue is reopened with a comment. One closed as not planned stays closed: that is how a
#   person dismisses a finding.
# - Any problem (GitHub unreachable or refusing, a missing setting, input that is not the JSON of
#   `cargo audit --json`) ends with an `::error::` line and exit status 1. The token is never printed.

set -eu

die() {
  echo "::error::$*"
  exit 1
}

[ $# -eq 1 ] || die "usage: sh scripts/report-advisories.sh AUDIT-JSON (the output of cargo audit --json)"
GITHUB_API=${GITHUB_API:-https://api.github.com}
GITHUB_REPO=${GITHUB_REPO:-rabalyn/shale}
[ -n "${GH_TOKEN:-}" ] || die "GH_TOKEN is not set or empty (the Forgejo Actions secret MIRROR_DEPLOY_KEY: a GitHub token with Issues read and write for $GITHUB_REPO)"
command -v curl >/dev/null 2>&1 || die "curl is not installed (needed to talk to GitHub)"
command -v jq >/dev/null 2>&1 || die "jq is not installed (needed to read the answers; apt-get install jq)"
[ -r "$1" ] || die "cannot read $1"
jq -e '.vulnerabilities.list | type == "array"' "$1" >/dev/null 2>&1 \
  || die "$1 is not the JSON of cargo audit --json (no list vulnerabilities.list)"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
at=${AUDIT_COMMIT:+ at $AUDIT_COMMIT}

# api METHOD PATH [JSON-BODY]: the answer's JSON is in $tmp/body, the status in $code
api() {
  url="$GITHUB_API/repos/$GITHUB_REPO$2"
  if [ $# -ge 3 ]; then
    code=$(http -X "$1" -H 'Content-Type: application/json' --data "$3" "$url") \
      || die "GitHub is not reachable: $1 $url"
  else
    code=$(http -X "$1" "$url") || die "GitHub is not reachable: $1 $url"
  fi
  case $code in
    2??) jq -e . "$tmp/body" >/dev/null 2>&1 || die "GitHub's answer for $1 $2 is not JSON" ;;
    404) [ "$1" = GET ] || die "GitHub answered 404 for $1 $2" ;;
    401 | 403) die "GitHub answered $code for $1 $2: the token may not write the issues of $GITHUB_REPO. The owner has to give the GitHub token in the Forgejo Actions secret MIRROR_DEPLOY_KEY the permission Issues (read and write) for $GITHUB_REPO." ;;
    *) die "GitHub answered $code for $1 $2" ;;
  esac
}

# curl with timeouts and no retries; the body goes to $tmp/body, the HTTP status is printed
http() {
  curl -sS --connect-timeout 10 --max-time 30 -o "$tmp/body" -w '%{http_code}' \
    -H "Authorization: Bearer $GH_TOKEN" -H 'Accept: application/vnd.github+json' \
    -H 'X-GitHub-Api-Version: 2022-11-28' "$@"
}

comment() { # NUMBER TEXT
  api POST "/issues/$1/comments" "$(jq -n --arg body "$2" '{body: $body}')"
}

ensure_label() { # NAME
  api GET "/labels/$(printf '%s' "$1" | jq -sRr @uri)"
  [ "$code" = 404 ] || return 0
  api POST /labels "$(jq -n --arg name "$1" '{name: $name, color: "b60205", description: "Reported by cargo audit (BUILD-021)"}')"
}

# --- 1. the findings, one per key
jq -c '
  def entry(kind): {kind: kind, adv: .advisory, pkg: "\(.package.name) \(.package.version)",
    patched: ((.versions.patched // []) | join(", "))};
  [(.vulnerabilities.list[] | entry("vulnerability")),
   ((.warnings // {}) | to_entries[] | .key as $k | .value[]? | entry(.kind // $k))]
  | map(. + {key: (if .adv then .adv.id else "yanked:\(.pkg | sub(" "; "@"))" end)})
  | group_by(.key) | .[]
  | {key: .[0].key, kind: .[0].kind, adv: .[0].adv, pkgs: (map(.pkg) | unique),
     patched: .[0].patched}' "$1" >"$tmp/findings"

# --- 2. the script's issues: labelled `security`, with a marker; the newest one per key
: >"$tmp/issues"
page=1
while :; do
  api GET "/issues?state=all&labels=security&per_page=100&page=$page"
  [ "$code" = 200 ] || die "GitHub answered $code for the list of issues"
  [ "$(jq length "$tmp/body")" -gt 0 ] || break
  jq -c '.[] | select(.pull_request == null)
    | (.body // "" | capture("<!-- shale-advisory:(?<key>[^ ]+) -->")) as $m
    | {number, state, reason: (.state_reason // ""), key: $m.key}' "$tmp/body" >>"$tmp/issues"
  page=$((page + 1))
done
jq -sc 'group_by(.key) | map(max_by(.number)) | INDEX(.key)' "$tmp/issues" >"$tmp/by_key"

# --- 3. every finding has an open issue, unless it was dismissed
did=0
while IFS= read -r f; do
  key=$(printf '%s' "$f" | jq -r .key)
  issue=$(jq -c --arg k "$key" '.[$k] // empty' "$tmp/by_key")
  if [ -n "$issue" ]; then
    n=$(printf '%s' "$issue" | jq -r .number)
    case $(printf '%s' "$issue" | jq -r '"\(.state):\(.reason)"') in
      open:*) continue ;;
      closed:not_planned) continue ;;
    esac
    api PATCH "/issues/$n" '{"state":"open"}'
    comment "$n" "cargo audit reports $key again$at. Close this issue as *not planned* to stop it from being reopened."
    echo "reopened #$n: $key is reported again"
    did=1
    continue
  fi
  kind=$(printf '%s' "$f" | jq -r .kind)
  ensure_label security
  ensure_label "security/$kind"
  title=$(printf '%s' "$f" | jq -r '"Security: \(.key) in \(.pkgs | join(", "))" + (if .adv.title then ": \(.adv.title)" else "" end)')
  body=$(printf '%s' "$f" | jq -r --arg at "$at" '
    "`cargo audit` reports a finding (\(.kind))\($at).\n\n"
    + "- Crate versions: \(.pkgs | map("`\(.)`") | join(", "))\n"
    + (if .adv then "- Advisory: \(.adv.id)" + (if .adv.url then " (\(.adv.url))" else "" end) + "\n"
         + (if .adv.date then "- Published: \(.adv.date)\n" else "" end)
         + (if (.adv.aliases // []) | length > 0 then "- Aliases: \(.adv.aliases | join(", "))\n" else "" end)
       else "" end)
    + (if .patched != "" then "- Fixed in: \(.patched)\n" else "" end)
    + (if .adv.description then "\n\(.adv.description)\n" else "" end)
    + "\nWhat to do: update the crate (`cargo update -p NAME`, or a newer version in `Cargo.toml`), replace it, or, when it cannot be fixed and does not matter for Shale, write down why and close this issue as *not planned*. The issue is closed by itself once `cargo audit` no longer reports the finding.\n\n"
    + "Made by `scripts/report-advisories.sh` (BUILD-021).\n\n<!-- shale-advisory:\(.key) -->"')
  api POST /issues "$(jq -n --arg title "$title" --arg body "$body" --arg kind "security/$kind" \
    '{title: $title, body: $body, labels: ["security", $kind]}')"
  echo "opened #$(jq -r .number "$tmp/body"): $title"
  did=1
done <"$tmp/findings"

# --- 4. an open issue whose finding is gone is closed
jq -r --slurpfile f "$tmp/findings" '($f | map(.key)) as $keys
  | .[] | select(.state == "open" and (.key | IN($keys[]) | not)) | "\(.number) \(.key)"' "$tmp/by_key" \
  >"$tmp/gone"
while read -r n key; do
  comment "$n" "cargo audit no longer reports $key$at."
  api PATCH "/issues/$n" '{"state":"closed","state_reason":"completed"}'
  echo "closed #$n: $key is no longer reported"
  did=1
done <"$tmp/gone"

[ "$did" -eq 1 ] || echo "nothing new ($(wc -l <"$tmp/findings" | tr -d ' ') findings, all reported)"
