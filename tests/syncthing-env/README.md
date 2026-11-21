# Syncthing test environment

Two [Syncthing](https://syncthing.net) devices (`docker.io/syncthing/syncthing:2.0`) in two rootless containers keep two host
folders in sync: "device A" and "device B". Shale's tests save in one folder and watch the other, to see how a cloud-folder
client treats Shale's files (NET-001, NET-002, NET-007..010, [`docs/storage-on-shares.md`](../../docs/storage-on-shares.md), #470).
No discovery, no relays, no internet: the devices find each other on a private podman network. Needs `podman` (or Docker:
`SHALE_SYNCTHING_ENV_ENGINE=docker`), `curl` and `jq`.

```sh
tests/syncthing-env/syncthing-env.sh up       # start both and wire them (a few seconds); the data is in tests/syncthing-env/data/
tests/syncthing-env/syncthing-env.sh status
tests/syncthing-env/syncthing-env.sh dirs     # the two vault folders, A then B
tests/syncthing-env/syncthing-env.sh pause    # pause the folder on both devices (so that edits on both conflict); `resume` undoes it
cargo test --bin shale -- --ignored syncthing_env --test-threads=1    # about 2 minutes (Syncthing rescans every 3 s and batches changes)
tests/syncthing-env/syncthing-env.sh down     # stop; `reset` also removes the data
```

The GUI of device A is http://127.0.0.1:18384 and of B http://127.0.0.1:18385 (API key `shale-env`).

The tests use one thread, a folder of their own per run (`t-<tag>-<pid>`) and remove it afterwards. They are not part of
`.forgejo/scripts/env-tests.sh` (not run by `land.sh` or in CI): nothing in Shale's code that they check changes with the
usual changes to the code, and a run takes minutes. The helper takes the machine-wide lock of the podman stacks
(`tests/env-lock.sh`, DEV-034) and does **not** pass it on to the containers (`7>&-`), or they would hold it for as long as they run.
