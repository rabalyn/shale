#!/bin/sh
# Tests for check-coverage.sh (DEV-003, DEV-010): the ratchet's pass and fail cases.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

# 2 files: 80 of 100 and 40 of 100 lines hit: 60.00 % in total
printf 'SF:a.rs\nLF:100\nLH:80\nend_of_record\nSF:b.rs\nLF:100\nLH:40\nend_of_record\n' > "$tmp/lcov.info"
fail=0
expect() { # name expected-status baseline
  echo "$3" > "$tmp/baseline"
  status=0
  sh "$here/check-coverage.sh" "$tmp/lcov.info" "$tmp/baseline" >/dev/null 2>&1 || status=$?
  if [ "$status" -ne "$2" ]; then echo "FAIL: $1 (exit $status, wanted $2)"; fail=1; else echo "ok: $1"; fi
}
expect "equal to the baseline passes" 0 60.00
expect "0.5 below the baseline passes" 0 60.50
expect "more than 0.5 below the baseline fails" 1 60.51
expect "above the baseline passes" 0 50
expect "a baseline that is no number is an error" 2 abc
printf 'SF:a.rs\nend_of_record\n' > "$tmp/lcov.info"
expect "an lcov file without lines is an error" 2 60
exit "$fail"
