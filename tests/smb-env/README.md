# Samba test environment

A real Samba server (`docker.io/dperson/samba`, smbd 4.12) in a rootless container, share `vault`, user `shale`, password `shale`,
on 127.0.0.1:1445. Shale's storage checks (`src/netshare_env_tests.rs`, NET-006, NET-011,
[`docs/storage-on-shares.md`](../../docs/storage-on-shares.md), #470) run in the share once it is mounted.

**The mount needs root.** A rootless user cannot mount CIFS with the kernel client (not in a user namespace either), so the stack stops at
the server unless you mount the share yourself, once, with `sudo`:

```sh
tests/smb-env/smb-env.sh up       # rootless: the server
tests/smb-env/smb-env.sh mount    # sudo mount -t cifs … on $XDG_RUNTIME_DIR/shale-smb-env-mnt-<uid>  (SHALE_SMB_ENV_VERS=2.1 for another protocol version)
cargo test --bin shale -- --ignored smb_env --nocapture    # prints the `NET observation` lines
tests/smb-env/smb-env.sh down     # umount (sudo) and stop
```

Not run on a machine without a kernel mount: the ignored test says so ("the Samba share is not mounted"). It is not in
`.forgejo/scripts/env-tests.sh` and cannot be in CI without a privileged runner (rabalyn/shale#477). What was run: `up`, and an
`smbclient` upload to the share from inside the container.
