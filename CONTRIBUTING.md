# Contributing

Shale is built entirely by AI coding agents; the owner decides what it does, sets the specs and
reviews the changes. Read the notice at the top of the [README](README.md) first. If you do not
want to take part in a project made this way, that is a fair choice.

Reports, ideas and changes from other people are welcome. The rules below are the rules the agents
follow too ([`docs/spec/engineering.md`](docs/spec/engineering.md), FORGE, DEV and BUILD).

## Reporting bugs and ideas

Use the issue forms on GitHub: <https://github.com/rabalyn/shale/issues/new/choose>. There is a
form for a bug and one for a feature request. One problem or idea per issue. Look for an open
issue first and add to it when there is one.

Every open to-do of Shale is a GitHub issue (FORGE-004), so the issue list is the plan. Do not
report a security problem there, see Security below.

## How changes are made

- **Issue first.** Work starts from an issue (FORGE-005). Commits name it: `Refs #N`, or
  `Closes #N` in the change that finishes it. `no-issue:` is only for a typo or a formatting fix.
  `scripts/land.sh` refuses a commit without one (FORGE-008).
- **Spec first** ([ADR-0001](docs/decisions/0001-spec-driven-development.md)). The behaviour is
  written as a row in [`docs/spec/`](docs/spec/README.md) before the code. The row is `SHOULD BE`
  while planned and becomes `IS NOW`, with the test name, in the change that makes it true. A test
  that checks a rule carries its ID in the name or in a `// spec: ID` comment.
- **No warnings** (DEV-001, BUILD-004). `cargo fmt --check`,
  `cargo clippy --all-targets --locked -- -D warnings` and `cargo test --locked` must be clean.
  Do not silence a warning with `#[allow]`; ask instead.
- **Every code path gets a sensible test** (DEV-003). Mail and CalDAV are tested against real
  servers in podman test stacks (DEV-005). If a good test is hard to write, propose developer
  tooling in an issue first (DEV-004).
- **Docs in the same change** (DEV-006, DEV-018). Check the README, `docs/FEATURES.md`, the specs,
  the user guide in `docs/userguide` and the screenshots, and update what the change made stale.
  Add an entry to `docs/changelog-pending/` (not to `CHANGELOG.md`).

## Pull requests

Pull requests are welcome, and optional. Shale's git and CI live on the owner's own Forgejo, and
the `main` on GitHub is a mirror of it, so a pull request is not merged on GitHub. The owner takes
your branch and lands it with `scripts/land.sh`, which merges it, runs the checks and pushes
`main` (FORGE-011, [ADR-0016](docs/decisions/0016-local-landing.md)). Your commits and their
authorship stay in the history. The pull request is then closed with a note.

The [pull request template](.github/pull_request_template.md) lists the checks. An issue with a
clear description is often as useful as a pull request.

## Labels

- `bug`: something does not work. `kind/feature`: user-visible behaviour. `kind/engineering`:
  code quality, tests, CI, tooling. `kind/process`: how the project works.
- `area/*`: the part of Shale (editor, mail, calendar, search, and so on).
- `platform/windows`: only on Windows. `performance`: speed and memory. `accessibility`: a barrier
  for people with disabilities.
- `security`: a security finding, such as one from `cargo audit` (BUILD-021).
- `question`: waits for the owner's decision.
- `proposal/tooling` and `tooling/accepted`: a tooling proposal and its acceptance (DEV-004).
- `forgejo-pr`: a pull request of the Forgejo era, imported as an issue (FORGE-012).

## Licence

Shale is free software under the GPL-3.0-or-later ([`LICENSE`](LICENSE),
[ADR-0024](docs/decisions/0024-gpl-licence.md)). Your contribution is under the same licence.

## Conduct

Be kind and civil. See the [code of conduct](CODE_OF_CONDUCT.md).

## Security

Report a vulnerability privately, not in a public issue. See [SECURITY.md](SECURITY.md).
