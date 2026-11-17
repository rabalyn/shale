# Security

## Reporting a vulnerability

Please do **not** open a public issue for a security problem. Report it privately on GitHub:
**Security → Report a vulnerability** in <https://github.com/rabalyn/shale> (GitHub's private
vulnerability reporting). Say what you found, which version or commit, and how to reproduce it.

The report stays private between you and the maintainer until a fix is released; the fix and the
advisory are then published together, naming you if you like. Shale is a one-person project
built by AI coding agents (see the README), so there is no fixed response time, but security
reports come first.

## What is covered

- The Shale program and its release files on GitHub (check them with `sha256sum -c` and
  `gh attestation verify`, see the README).
- Its dependencies: `cargo audit` runs daily and every advisory becomes an issue labelled
  `security` (BUILD-021); GitHub's Dependabot alerts and secret scanning are on (BUILD-022).

Data safety rules of the program (where notes, passwords and tokens are kept) are in
[`docs/spec/vault-and-files.md`](docs/spec/vault-and-files.md) (VAULT) and [`docs/spec/features.md`](docs/spec/features.md) (SEC) and
[ADR-0003](docs/decisions/0003-data-safety.md).
