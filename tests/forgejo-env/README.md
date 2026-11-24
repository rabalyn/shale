# Forgejo test environment

A real Forgejo (SQLite) in one container, for the server path of the sync wizard (SYNC-066, rabalyn/shale#469).
It listens on `127.0.0.1:13000` (web and API) and `127.0.0.1:13022` (Forgejo's built-in SSH server, SYNC-081) only, has
no registration and no mail, Actions enabled (no runner unless a test starts one, see below), and one seeded administrator: user `shale`, password `shale-forgejo-test-1` (a test account of a throw-away server). The tests make
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
- the words for a wrong token, a repository that is not there and no server;
- SSH (SYNC-081, SYNC-083): the test makes keys with `ssh-keygen` in a temporary folder and registers them through
  `POST /api/v1/user/keys`; a key without a passphrase syncs a vault; a key with a passphrase syncs when it is in an
  `ssh-agent` the test starts on a socket of its own (`ssh-add` gets the passphrase from a throw-away `SSH_ASKPASS`
  script, in the test only); without it the failure is told as "locked" or "no agent". The clone address is
  `ssh://git@127.0.0.1:13022/<owner>/<repo>.git` and the tests trust the host in a `known_hosts` file of their own.

- the publish of a vault as a Hugo site (SITE-062 to SITE-075, `src/site_export/publish/env_tests.rs`): over HTTPS with a token and
  over SSH one parentless snapshot replaces the last one (an unpublished note is in no commit of the server), a wrong token or a
  missing repository changes nothing and is told in plain words, a branch of another vault is taken over only when confirmed, and
  media go to the server's LFS store (`LFS_START_SERVER` in `compose.yaml`) once.

- the Forgejo Actions workflow of the server recipe in the user guide (SITE-049, #665, `src/site_export/publish/recipe_env_tests.rs`): the
  test starts a runner (`forgejo-env.sh runner-up <folder>`, `runner.yaml`, an image of `runner/Containerfile`: the official runner on Debian
  with Node.js, git, git-lfs and curl; its `docker` label is a `host` label, so the jobs run in the runner's container and no nested containers
  are needed; it registers with a token from the administrator's API and has the folder as `/srv/www`), pushes the note's own workflow to a
  second repository, publishes a vault with a song to the first, presses *Run workflow* through the API and checks the folder the workflow
  built: the page, the LFS file itself (not its pointer), the Pagefind index, the atomic swap, and that a press with nothing new builds nothing.
  The first run builds the image (a few minutes), and the workflow downloads Hugo and Pagefind from GitHub, as it would on a server.
  `forgejo-env.sh job-log <owner/repo>` prints the jobs' log (needs `zstd`); the test prints it when a run fails.

The tests find the Shale executable that cargo built (`target/debug/shale`; `SHALE_ENV_TESTS_BIN` names another).
The container image is `codeberg.org/forgejo/forgejo:11.0.16-rootless`, pinned. With Docker the compose command is
`SHALE_FORGEJO_ENV_COMPOSE="docker compose"`. `up`, `down` and `reset` take the machine-wide lock of the podman stacks
(`tests/env-lock.sh`, DEV-034).

In CI the `env-tests` job runs it with the other suites (`.forgejo/scripts/env-tests.sh`).
