# ADR-0021: The current TeX Live is checked weekly and for releases, not on every tag

- **Status**: Accepted (owner decision, 2026-11-18)
- **Date**: 2026-11-18
- **Amended by**: [ADR-0026](0026-weekly-only-texlive-refresh.md), release tags no longer refresh the TeX image
- **Amends**: [ADR-0020](0020-current-texlive.md), the consequence that the CI job (tags and weekly) refreshes the image and runs the real install
- **Specs**: DEV-011 ([`docs/spec/engineering.md`](../spec/engineering.md)), TEX-020 ([`docs/spec/tex-export.md`](../spec/tex-export.md))
- **Issues**: #325

## Context

The `env-tests` job of a tag run grew from about 4 minutes (v0.4.0-rc.1) to 23 minutes (v0.5.0,
run #363). The causes:

- the TeX image is built with `--no-cache` (`tex-env.sh build --refresh`): the current TeX Live and 67
  packages are downloaded from CTAN every time;
- `tex_install` does a real managed install (about 100 MB);
- `tex_import` downloads templates from GitHub and CTAN;
- podman runs with the slow `vfs` storage in the job container;
- the runner is arm64.

ADR-0020 wants a change of the current TeX Live to be noticed; that does not need a check on every tag.

## Decision

`SHALE_ENV_TESTS_TEX_REFRESH`, `SHALE_ENV_TESTS_TEX_INSTALL` and `SHALE_ENV_TESTS_TEX_IMPORT` are `yes`
in the weekly scheduled run and in release tags (`vX.Y.Z`, no pre-release part) of the `env-tests` job,
and `no` in check tags (`vX.Y.Z-rc.N`). A check tag builds the TeX image without `--refresh` and runs
only `calendar::env_tests`, `mail_env` and `tex_env`. The condition is
`(github.event_name == 'schedule' || (startsWith(github.ref, 'refs/tags/v') && !contains(github.ref_name, '-'))) && 'yes' || 'no'`. `scripts/land.sh` is unchanged.

## Consequences

- A change of the current TeX Live (or of CTAN's templates) shows up within a week, as ADR-0020 meant,
  and always before a release, since release tags run the full check.
- Check tags are fast.
- Podman storage on a runner volume (for example in the runner's `valid_volumes`) would let images and
  layers survive between runs and would also drop the image build from tag runs. It needs the owner to
  change the runner's configuration and is not part of this decision; it is a separate change.
