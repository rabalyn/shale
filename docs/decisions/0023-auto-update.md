# ADR-0023: Shale updates itself from GitHub releases, when the person wants it

- **Status**: Accepted (owner request, 2026-11-19)
- **Date**: 2026-11-19
- **Specs**: UPD-001 to UPD-022 ([`docs/spec/updates.md`](../spec/updates.md))
- **Issues**: #421; the authenticity of a download beyond its checksum is #422

## Context

Shale is published as a Linux tarball (installed with `install.sh` or run from its folder), a macOS
dmg with `Shale.app`, a Windows zip, and for Arch as the AUR package `shale-bin`
(`packaging/aur/shale-bin/`). Every release asset has a `.sha256` file next to it (BUILD-012). A
person who installed Shale from a download has to notice a new release, download it and repeat the
install by hand. Some people do not want a program that talks to the internet on its own, and a
package manager must stay the owner of the files it installed.

## Options

1. **Package managers only**: no code; people on the tarball, the dmg and the zip never hear of a
   new release.
2. **A crate such as `self_update`**: it does check, download and replace, but brings an HTTP client
   and archive stack of its own (a second TLS stack next to the system's `curl`), knows nothing about
   `install.sh`, the AUR or the macOS bundle, and replaces files without the "keep the old version"
   step.
3. **Our own small updater** on what Shale already has: `curl` for HTTPS (as CalDAV and the TeX Live
   download do), `ring` for SHA-256, the archive reader of the TeX templates for `.tar.gz` and `.zip`,
   background jobs, and `serde_json`.

## Decision

Option 3, in `src/update/`, with these rules:

1. **The person decides** (`updates.mode`): `Ask` (the default; the first time an update is found a
   dialog asks), `Off`, `Notify`, `Download`, `Install`. `Off`, and `SHALE_UPDATES=off` in the
   environment (for packagers and tests), mean **no network request of any kind** for updates and no
   notice.
2. **One request per check**: `GET https://api.github.com/repos/rabalyn/shale/releases/latest`
   through `curl`, https only; drafts and pre-releases are ignored. Checks run a short while after
   the start and every 24 hours.
3. **Integrity**: the asset and its `.sha256` come from the same release; the SHA-256 must match or
   nothing is installed and the download is deleted.
4. **Install kinds are detected from the path of the running program** and each has its own rule: an
   `install.sh` install runs the new `install.sh` again; an unpacked tarball or zip is replaced in
   place; a `Shale.app` bundle is swapped; a package-managed program (`/usr/bin`, the AUR) and a
   development build (`target/`) are **never touched**: they only get a notice. A folder without
   write access turns Install into Download with the command to run.
5. **The previous version is kept** next to the new one where that is cheap (`shale.old`), and
   removed after the new version has run; Windows removes `shale.exe.old` at the next start.
6. **Restart is the person's choice**: "Restart now" saves what quitting saves, then starts the new
   program, which waits for the lock of the single instance (DATA-014) to be free.

## Consequences

- **A checksum proves integrity, not authenticity.** It comes from the same release as the file, so
  it catches a damaged or truncated download, not a compromised release or account. Authenticity
  (the build attestation, or a signature with a key built into Shale) is #422 and stays a SHOULD BE
  row (UPD-022). Until then the safest settings are `Notify` or `Off`.
- Package-managed installs are never replaced; the notice names the package manager's command.
- `Off` is a hard off: no request, no banner, no timer.
- macOS builds are ad-hoc signed, so the Keychain may ask again after an update.
- The code of the macOS and Windows installs is checked by clippy on Linux and by its plan (a list of
  file operations, a pure function that is tested for every kind), but runs for real only on those
  systems.
- A shared download helper (`src/download.rs`) replaces the TeX Live install's private one.
