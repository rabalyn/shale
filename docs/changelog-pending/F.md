### Added
- Release downloads come with a SHA-256 checksum file (`shale-linux-x86_64.tar.gz.sha256`, on the "latest" build and every release page); check a download with `sha256sum -c`. It catches a damaged download, not a compromised release page.
- A separate "audit" job in the build workflow runs `cargo audit` on the dependencies, on every push and weekly; a new advisory fails only that job and never blocks a release build.
