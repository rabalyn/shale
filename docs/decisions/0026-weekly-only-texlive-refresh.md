# ADR-0026: Only the weekly run rebuilds the TeX image without the cache; no coverage job on release tags

- **Status**: Accepted (owner decision, 2026-11-21)
- **Date**: 2026-11-21
- **Amends**: [ADR-0021](0021-weekly-texlive-check.md), the decision that release tags also refresh the TeX image (`SHALE_ENV_TESTS_TEX_REFRESH=yes`)
- **Specs**: DEV-010, DEV-011 ([`docs/spec/engineering.md`](../spec/engineering.md))
- **Issue**: #455

## Context

A tag takes about 22 minutes on Forgejo's single runner (build 4.5, coverage 3.5, env-tests 13.5,
one after another). The `env-tests` job of a release tag takes 13.5 minutes where the same suites
take about 6 minutes locally; most of the difference is the TeX image built without the cache
(`tex-env.sh build --refresh`: the current TeX Live and its packages from CTAN every time).

A release tag (`vX.Y.Z`) follows a check tag (`vX.Y.Z-rc.N`) of the same code (FORGE-014), and the
weekly run already notices a changed TeX Live within a week (ADR-0020).

## Decision

1. `SHALE_ENV_TESTS_TEX_REFRESH` is `yes` only in the weekly scheduled run. Check tags and release tags
   build the TeX image as cached (ADR-0022). Release tags still run the real TeX Live install and the
   template import (`SHALE_ENV_TESTS_TEX_INSTALL`, `SHALE_ENV_TESTS_TEX_IMPORT`, ADR-0021).
2. The `coverage` job runs for check tags only (`vX.Y.Z-rc.N`), not for release tags; it did not run
   for the weekly run or a manual dispatch before. `mirror` needs only `build`, so a skipped
   `coverage` job holds up nothing.

Raising the runner capacity so that jobs run side by side is the owner's change to the homelab runner
and not part of this decision.

## Consequences

- A release tag takes about the time of its slowest job.
- A changed TeX Live shows within a week, not necessarily at the release; a release follows a check
  tag, and the check tag and the weekly run are the earlier checks.
- A coverage drop is seen at the check tag; the release of the same code is not measured again.
