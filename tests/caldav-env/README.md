# CalDAV test environment

A local CalDAV server ([Radicale](https://radicale.org/)) behind TLS with a seeded user and
calendars, to test Shale's calendar code against a real server and to try it by hand (CALENV-001,
DEV-005). It listens on `127.0.0.1:15232` only. The credentials below are test values; the
certificate authority is made here and is for this environment only.

Needs `podman-compose` (or `docker compose`), `openssl` and `curl`. The images are pinned:
`docker.io/tomsquest/docker-radicale:3.5.4.0` and `alpine:3.24.2` (for seeding).

## Start and stop

```sh
tests/caldav-env/caldav-env.sh up       # certificates (first time), containers, seeded calendars
tests/caldav-env/caldav-env.sh status
tests/caldav-env/caldav-env.sh down     # stop (the calendars stay in the volume)
tests/caldav-env/caldav-env.sh reset    # stop and delete the calendars; the next `up` seeds them again
```

`SHALE_CALDAV_ENV_COMPOSE="docker compose"` selects Docker. The seeded events are dated relative
to the first `up` (a few days around today, so they are inside the window Shale loads, 4 months
back to 13 months ahead), and the certificate lasts a year: after a long time, `rm -r
tests/caldav-env/certs`, then `reset` and `up`.

## The account and calendars

User name `shale`, password `shale-test-pw`. Server address `https://localhost:15232/` (Shale finds
the calendars by itself), or a calendar directly:

| Calendar | Address | Holds |
|---|---|---|
| Work | `https://localhost:15232/shale/work/` | Standup and Design review (single events), "Übergabe: Café & Größe" (non-ASCII title), "Call with reminder" (a `VALARM` 15 minutes before), "Company day" (all-day), "Weekly sync" (weekly, 6 times starting two weeks ago, one date skipped with `EXDATE`, one date moved and renamed with `RECURRENCE-ID`) |
| Private | `https://localhost:15232/shale/private/` | Doctor, Birthday (all-day) |
| Moves | `https://localhost:15232/shale/moves/` | empty: a target for moving events |

The data is in `seed/data/` (`.ics` files with day markers like `@+3@` that `seed/seed.sh`
turns into dates when it fills the volume).

A resource **without ETag** cannot be made: Radicale sends a version tag for everything. Shale's
no-tag path (CAL-016: fetch and compare before saving or deleting) is tested against it by passing
no tag. A server that really sends none would need a small proxy that strips the `ETag` header;
not built.

## Trust the certificate

Shale checks server certificates through `curl` with the operating system's trust store and has
no switch to skip that. The test CA is `tests/caldav-env/certs/ca.crt`. Either:

- start Shale with `SSL_CERT_FILE=$PWD/tests/caldav-env/certs/ca.crt shale` (or `cargo run`):
  that run trusts **only** this CA, so other HTTPS sites fail in that run; or
- add the CA to the system store for as long as you test (Arch: `sudo trust anchor --store
  tests/caldav-env/certs/ca.crt`, Debian/Ubuntu: copy it to `/usr/local/share/ca-certificates/`
  and run `sudo update-ca-certificates`) and remove it afterwards.

Then Settings, Calendars, CalDAV: server `https://localhost:15232/`, user `shale`, password
`shale-test-pw`, Find calendars.

## The automated tests

```sh
tests/caldav-env/caldav-env.sh up
cargo test -- --ignored caldav_env
```

Seven tests in `src/caldav_env_tests.rs` (ignored by default, so `cargo test` never needs
containers): discovery and loading of the seeded calendars; create, edit and delete; changing and
removing one date of a repeating event; moving between calendars; a change made on the server
meanwhile (the save and the delete are refused, with and without a version tag); a wrong
password; and the https, redirect and certificate rules (without the test CA the certificate is
refused). They take turns, give every event they make a unique UID starting with `caldav-env-` and
remove what they made (also what a failed earlier run left), so they can be run again at once; the
seeded events are only read. The tests trust the CA through `cal_dav::TEST_CA`, which exists only
in test builds. Use `SHALE_CALDAV_ENV_COMPOSE` as above for Docker.

## In CI

The `env-tests` job of `.forgejo/workflows/ci.yml` (DEV-011, CALENV-004, MAIL-093) runs both this
suite and the mail suite (`tests/mail-env/`) with `.forgejo/scripts/env-tests.sh run`: on pushes to
`main`, on pull requests that touch `src/cal_*`, `src/caldav_*`, `src/imap*`, `src/mail*`,
`src/smtp*` or `tests/{caldav,mail}-env/`, and weekly. It starts both stacks, runs both ignored
suites (`--test-threads=1`, and fails if a suite ran no test) and always takes the stacks down.

The job runs in the `rust` container like the other jobs, so it needs a container engine **inside**
that container. It installs `podman` with apt and `podman-compose` 1.2.0 with pip (bookworm's 1.0.3 lacks
`network_mode: "service:…"`) and then asks `podman info`
(then `docker info`) and starts a small `alpine` container with it (an unprivileged job
container answers `info` but cannot unpack images). If neither works, the job prints `SKIPPED: this runner cannot run containers
…` and ends green: the real-server tests did not run. What the runner must offer for them to run:

- the job container must be allowed to run containers: for the runner's `container.options` in the
  `act_runner` configuration, `--privileged` (and a rootful or a user-namespace-capable Podman; the
  image storage inside the job container is not shared, so every run pulls the images again), **or**
- a Docker socket mounted into the job container (`container.valid_volumes` and
  `container.options: -v /var/run/docker.sock:/var/run/docker.sock`, then install `docker.io` and
  `docker-compose`) **and** host networking for the job container (`container.network: host`), because
  the stacks publish their ports on the host's `127.0.0.1` and the tests connect to them there; the
  repository's bind mounts then also need the same path on the host, which is why the first way is
  the supported one, **or**
- a runner without the container layer: a job with a label that runs on the host itself (a `host`
  label whose runner has Podman, `openssl`, `curl` and a Rust toolchain). Then change `runs-on` of
  `env-tests` to that label and drop the `container:`.

Inside the job container Podman stores images with the `vfs` driver (the job writes
`/etc/containers/storage.conf` before Podman first runs): its default `overlay` driver does not work on top of the job container's own overlayfs.

The job has not yet been confirmed on the homelab runner (it is only known to work where Podman
works, e.g. a developer machine: run `sh .forgejo/scripts/env-tests.sh run`).

## How it is built

- `compose.yaml`: `seed` (Alpine, once) copies `seed/data/` into the volume with the dates filled
  in, only when the volume is empty; `radicale` serves it.
- `radicale/config`: TLS from `certs/`, `htpasswd` authentication with the plain test password in
  `radicale/users`, owner-only rights (the user sees only `/shale/`), no web interface.
- `setup-certs.sh`: a CA and a server certificate for `localhost` / `127.0.0.1` into `certs/`
  (git-ignored).
