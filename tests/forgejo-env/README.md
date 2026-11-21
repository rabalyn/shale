# Forgejo test environment

A real Forgejo (SQLite) in one container, for the server path of the sync wizard (SYNC-066, rabalyn/shale#469).
It listens on `127.0.0.1:13000` only, has no registration, no SSH, no mail and no actions, and one seeded
administrator: user `shale`, password `shale-forgejo-test-1` (a test account of a throw-away server). The tests make
their own access tokens through the API (`POST /api/v1/users/shale/tokens`).

```sh
tests/forgejo-env/forgejo-env.sh up      # start, wait, make the administrator
cargo test -- --ignored forgejo_env      # the suite (one test thread is enough)
tests/forgejo-env/forgejo-env.sh reset   # stop and delete the data
```

What the suite checks (`src/gitsync/wizard/forgejo_env_tests.rs`):

- the API calls of the wizard: who the token belongs to, create a private repository, list, the clash of a second create,
  a wrong token;
- a real `git` syncs a vault with it, and git gets the token through **Shale's askpass helper** (the Shale executable
  with `SHALE_GIT_ASKPASS=1`, SYNC-054) from a pretend `secret-tool` (`tests/askpass/secret-tool`, a few lines of shell
  that keep secrets in a folder), as it would from a desktop's keyring;
- the words for a wrong token, a repository that is not there and no server.

The tests find the Shale executable that cargo built (`target/debug/shale`; `SHALE_ENV_TESTS_BIN` names another).
The container image is `codeberg.org/forgejo/forgejo:11.0.16-rootless`, pinned. With Docker the compose command is
`SHALE_FORGEJO_ENV_COMPOSE="docker compose"`. `up`, `down` and `reset` take the machine-wide lock of the podman stacks
(`tests/env-lock.sh`, DEV-034).

In CI the `env-tests` job runs it with the other suites (`.forgejo/scripts/env-tests.sh`).
