# ADR-0022: The env-tests job keeps podman's storage on a runner volume

- **Status**: Accepted (owner decision, 2026-11-18)
- **Date**: 2026-11-18
- **Amends**: [ADR-0021](0021-weekly-texlive-check.md), the consequence that podman storage on a runner volume is not part of that decision
- **Specs**: DEV-011 ([`docs/spec/engineering.md`](../spec/engineering.md)), TEX-020 ([`docs/spec/tex-export.md`](../spec/tex-export.md))
- **Issues**: #325

## Context

After ADR-0021 a tag run no longer builds the TeX image with `--no-cache`, but podman inside the job
container keeps its images in the job container itself, which is thrown away after the run. So every
run still built the TeX image from scratch (TeX Live and 67 packages from CTAN) and pulled the CalDAV
and mail images again. The homelab runner now offers a persistent directory for this
(`/srv/data/deployments/forgejo-runner/cache/shale/containers`, allowed in its `valid_volumes`; it runs
one job at a time, so no two runs share it at once).

## Decision

The `env-tests` job mounts that directory as `/var/lib/containers`, podman's `graphroot` (the `runroot`
stays in the job container). The storage driver stays `vfs`. After the tests, whatever their result,
the job prunes dangling images (`podman image prune -f`) and logs `podman system df`.

## Consequences

- A check tag run reuses the TeX image built by an earlier run and the pulled test images; the weekly
  run and a release tag still rebuild the TeX image without the cache (ADR-0021), and the prune then
  drops the old one.
- Containers, pods and volumes a cancelled run left behind are in the kept storage too;
  `env-tests.sh run` already resets both stacks (`down -v`) before it starts them.
- The storage driver cannot change without emptying the directory on the runner (a different driver
  cannot read the `vfs` store). Emptying it is always safe: the next run fills it again.
- The job depends on the runner offering this path. A runner that does not allow it fails the job
  at its start; the path is in the job, like the cargo caches of the other jobs.
