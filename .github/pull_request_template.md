<!-- GitHub fills this in for every new pull request (DEV-009). Delete what does not apply and say why. -->

Closes #

## What changed and why

## Checklist (DEV-006, ADR-0001)

- [ ] The issue is named here and in every commit (`Refs #N` / `Closes #N`).
- [ ] Spec rows first: new or changed behaviour has its row in `docs/spec/`; rows that became true are flipped from SHOULD BE to IS NOW with the test name, and the issue of a flipped row is closed.
- [ ] Tests carry the spec ID in their name or a `// spec: ID` comment.
- [ ] `cargo fmt --check`, `cargo clippy --all-targets --locked -- -D warnings` and `cargo test --locked` are clean, with no warning and no `#[allow(…)]`.
- [ ] Docs checked and updated where the change made them stale: `README.md`, `docs/FEATURES.md`, `docs/IMPLEMENTATION.md`, the specs.
- [ ] Screenshots checked: pictures the change made stale are made again with `docs/screenshots/regen.sh <name>` (and looked at), a new screen has a recipe in `docs/screenshots/recipes.conf` and is shown in the docs.
- [ ] A changelog entry is in `docs/changelog-pending/` (not in `CHANGELOG.md`).
