### Internal

- **Tag runs of the `env-tests` job reuse the TeX image** (#325). Podman's storage of the job is kept on
  a volume of the homelab runner (ADR-0022), so a tag run no longer builds the TeX Live image and pulls
  the CalDAV and mail images again; unused images are pruned after every run.
