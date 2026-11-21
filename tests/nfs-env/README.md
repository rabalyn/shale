# NFS test environment

A real NFS server (the kernel's nfsd, in the privileged container `docker.io/erichough/nfs-server` run with `sudo podman`) exporting
`tests/nfs-env/data/export` as NFSv4, and the kernel client mounting it. Shale's storage checks (`src/netshare_env_tests.rs`, NET-006,
NET-012, [`docs/storage-on-shares.md`](../../docs/storage-on-shares.md), #470) run in the mount.

**Everything here needs root**: nfsd lives in the kernel (the module `nfsd` must be loaded) and a mount needs `sudo`. It cannot run rootless
and cannot run in CI without a privileged runner (rabalyn/shale#477). **This stack has never been started**: the script was written and
syntax-checked on a machine where no `sudo` was used for it. Expect to fix details of the image's options on the first run.

```sh
tests/nfs-env/nfs-env.sh up       # sudo podman run … (privileged)
tests/nfs-env/nfs-env.sh mount    # sudo mount -t nfs4 …  (SHALE_NFS_ENV_OPTS=actimeo=3 or vers=… …: other mount options)
cargo test --bin shale -- --ignored nfs_env --nocapture    # prints the `NET observation` lines
tests/nfs-env/nfs-env.sh down     # umount and stop
```

The interesting runs are with different mount options (`actimeo=60` against `noac`): the attribute cache decides how soon one client sees
another's change (NET-002).
