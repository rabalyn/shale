# ADR-0031: Performance gates only on the workstation, statistics on GitHub

- **Status**: Accepted (owner decision, 2026-11-19)
- **Date**: 2026-11-19
- **Specs**: BUILD-034, BUILD-041, FORGE-014
- **Issue**: #408, #493
- **Amends**: [ADR-0018](0018-tag-only-ci.md) (the release step), [ADR-0025](0025-github-checks-main.md) (what runs on GitHub)

## Context

- A release measures its performance in `scripts/bench-release.sh` (BUILD-034) and stops on a
  regression against the previous release's stored result.
- The v0.7.0 release flagged 7 and then 9 time regressions that were all noise: the stored baseline
  came from another day, each side had two runs, and the computer was busy. Interleaved reruns of
  the flagged scenarios with 5 runs found none (#493).
- Shared public runners (GitHub, the Forgejo runner, an aarch64 machine used by every job) are
  noisier still and change their hardware between runs; a gate there would stop releases for noise.

## Decision

1. **The only gate is the owner's workstation**, in the release step before the tag. A flagged *time*
   regression is confirmed before it counts: the flagged scenarios alone are run again against the
   previous release, X and Y interleaved, 5 runs (`scripts/bench-compare.sh --filter`), and the
   release stops only if the regression shows again there. Allocations and peak bytes are exact and
   need no confirmation. The numbers of the re-run go into the release issue and into
   `docs/perf/compare/`, whatever they say. `--allow-perf-regression` stays for the owner.
2. **GitHub measures statistics, never a gate.** After the release page is up a job runs the `full`
   profile on Linux, macOS and Windows, keeps each result as an asset of the release, compares with
   the previous release's asset of the same system and writes the odd findings into the job
   summary. Nothing waits for it, and it cannot fail the workflow.
3. **Forgejo measures nothing** (its runner is an aarch64 machine shared by every job; the numbers
   would not be comparable to any other).

## Consequences

- A release on a busy workstation takes longer by the re-run of the flagged scenarios only.
- A real regression is found where the measurement is the most trustworthy; the GitHub numbers are
  a history across systems, read by a person.
- The stored results stay in git as the history (`docs/perf/results*/`).

## Alternatives

- **Gate on GitHub or Forgejo**: noise would stop releases, and a rerun there is as noisy.
- **Always rerun everything**: about an hour a release for a problem that exists in a handful of
  scenarios.
