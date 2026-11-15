#!/bin/sh
# DEV-003 ratchet: fails when the total line coverage of an lcov file is more than 0.5 points
# below the number stored in the repository. Usage: check-coverage.sh <lcov.info> <baseline-file>
# The baseline file holds one number (percent, e.g. 61.37); raise it by hand, see docs/coverage.md.
set -eu
lcov=${1:?usage: check-coverage.sh <lcov.info> <baseline-file>}
baseline_file=${2:?usage: check-coverage.sh <lcov.info> <baseline-file>}
tolerance=0.5

baseline=$(tr -d ' \n' < "$baseline_file")
case $baseline in
  ''|*[!0-9.]*) echo "$baseline_file does not hold a number: '$baseline'" >&2; exit 2 ;;
esac

# LF = instrumented lines, LH = hit lines, summed over all files
total=$(awk -F: '/^LF:/ { lf += $2 } /^LH:/ { lh += $2 } END { if (lf == 0) { print "none" } else { printf "%.2f", 100 * lh / lf } }' "$lcov")
if [ "$total" = none ]; then echo "no lines in $lcov" >&2; exit 2; fi

echo "Total line coverage: $total % (baseline $baseline %, tolerance $tolerance points)"
if awk -v t="$total" -v b="$baseline" -v tol="$tolerance" 'BEGIN { exit !(t < b - tol) }'; then
  echo "Coverage fell more than $tolerance points below the baseline: add tests, or (only with a reason) lower $baseline_file." >&2
  exit 1
fi
if awk -v t="$total" -v b="$baseline" 'BEGIN { exit !(t >= b + 1) }'; then
  echo "Coverage is 1 point or more above the baseline: raise $baseline_file to $total (docs/coverage.md)."
fi
