# ADR-0037: Release files are signed by the owner with an SSH ed25519 key, and the updater requires the signature

- **Status**: Accepted (owner decision, 2026-11-22: ed25519 key, public half built into Shale, verified with `ring`)
- **Date**: 2026-11-23
- **Specs**: UPD-022, UPD-023, UPD-024, UPD-025 ([`docs/spec/updates.md`](../spec/updates.md)), BUILD-014
- **Issue**: #422
- **Builds on**: [ADR-0023](0023-auto-update.md) (auto-update), [ADR-0018](0018-tag-only-ci.md) (tags and releases)

## Context

The updater of #421 checks a download against the `.sha256` file of the same GitHub release. That
shows the file arrived whole, nothing more: someone who got into the GitHub account, or a
compromised release workflow, replaces the file and its checksum together, and every Shale with
updates on (Install) runs the new program. The build attestation (BUILD-014) shows that the
workflow built a file, but Shale cannot check it without a Sigstore client, which means new
dependencies and a trust root that is GitHub's own.

The owner made an ed25519 key pair for this (OpenSSH format; the private key stays on his
computer) and decided: sign the release files, build the public key into Shale, verify with `ring`
(already a dependency; no new crypto dependency).

Facts about the release flow (FORGE-014, BUILD-012, BUILD-016): `make-release.sh` only tags;
Forgejo's CI passes; its `mirror` job pushes the tag to GitHub; GitHub **then** builds the files and
publishes the release page. The files do not exist when `make-release.sh` runs, so it cannot sign
them, and the private key must not go to CI.

## Decision

1. **Format: SSHSIG.** The key is an OpenSSH key, so the signer is `ssh-keygen -Y sign -f <key> -n
   <namespace> <file>` (OpenSSH's PROTOCOL.sshsig), which writes `<file>.sig`. Shale parses the
   armored blob and checks, in this order: the magic `SSHSIG`, version 1, that the public key in
   the signature is the built-in one, that the namespace is the expected one, that the hash
   algorithm is `sha512` (nothing else is accepted), and the ed25519 signature (`ring`,
   `UnparsedPublicKey::verify`) over the data that PROTOCOL.sshsig defines, rebuilt from the
   namespace, the reserved field, the hash algorithm and the SHA-512 of the file (read in chunks;
   the files are up to 400 MB). A cut blob, trailing data, another key type or bad base64 is an
   error in words, never a panic. A tool like minisign would have needed a second key; the owner's
   key and the standard `ssh-keygen` are enough, and a person can check a download by hand with
   `ssh-keygen -Y verify`.
2. **The namespace names the file**: `shale-release/<file name>`, for example
   `shale-release/shale-0.7.0-linux-x86_64.tar.gz`. SSHSIG signs the content only; without the
   name in the signed data, an old validly signed release file could be put on a new release (a
   downgrade, or a version with a known hole). With the name, a signature fits only that file of
   that version and system.
3. **The key is built in as the `.pub` text.** `src/update/release-signing.pub` (the OpenSSH line,
   copied from the owner's `.pub`) is `include_str!`ed and parsed at run time by a small tested
   function (the wire format: `string "ssh-ed25519"`, `string key[32]`). One file is the single
   source: the same text is what the user guide gives for a check by hand, and
   `scripts/sign-release.sh` compares the owner's key with it before signing. A second copy as a
   byte array would need a test to keep it in step. The parse cannot fail in a build that passed the
   tests (a test pins it), so `release_key()` may `expect`. Changing the key means changing this
   file and releasing a version; every Shale that has the old key only notifies for a release signed
   with the new one (it fails closed), so the key is changed with a release signed by the old key
   that carries the new one.
4. **Where signing happens: by the owner, after GitHub published.** `scripts/sign-release.sh vX.Y.Z`
   runs on the owner's computer. It checks that the key is the built-in one, downloads each file
   that has a `.sha256` with `gh release download`, checks it against the `.sha256` **and against
   the build attestation** (`gh attestation verify`, as the workflow does), signs it, verifies the
   signature with `ssh-keygen -Y verify`, and uploads the `.sig` files with `gh release upload`. The
   attestation check is what ties the signature to "built by the workflow from this tag": the owner
   signs what the workflow built, not whatever is on the page when he runs the script. The key
   path is `$SHALE_SIGNING_KEY` (default `~/.ssh/shale_github_signing`). Never in CI (the key
   would be a secret on a runner, which is what the signature is meant not to depend on), never
   in `land.sh`, `make-release.sh` or a test against the real GitHub; a test (`tests/release_scripts.rs`)
   fails if any of those scripts or workflows names the script or the key. The script's own test
   uses a stub `gh` and a throwaway key. `make-release.sh` does not wait for the release and sign:
   the GitHub build takes tens of minutes, the passphrase of the key is asked on the owner's
   terminal, and a script that waits that long and then fails for a typed passphrase is worse than
   one command the owner runs when he sees the release page.
5. **The updater's rule.** "Install automatically" and "Download" need a valid signature for the
   file of this system; the `.sha256` check stays and comes first (a damaged file is reported as
   damaged, not as badly signed).
   - **No `.sig` on the release** (older releases; the window between GitHub publishing and the
     owner signing): no automatic download or install, in any mode. It is the same path as a release
     without a file for this system (UPD-005): announced, with the reason and the release page in the
     notice. The next check (24 hours, or "Check for updates now") offers the install once the
     signature is up. The alternative, installing unsigned releases, would make the signature worth
     nothing: whoever can replace a file can also leave its `.sig` out.
   - **A bad signature** (file changed, other key, other file name): the download and the signature
     are deleted, nothing is installed, the update fails with a warning that names the file and the
     reason (the error notice with "Try again"), and a kept download from before is never trusted
     without checking it against the signature fetched now.
6. **Not decided here**: rotating or revoking the key (a new key needs a release that carries it, see
   3; there is no revocation list, a stolen key means a release with a new key and a note);
   signing the user guide files (the script signs them too, as they have a `.sha256`, but Shale does
   not use them). A timestamp or version floor against replaying an older signed release is not
   needed: the version is in the file name, the file name is in the namespace, and Shale offers only
   newer versions.

## Consequences

- Every release needs one more step by the owner (below); until then Shale users on Install only get
  a notice. A release the owner forgot to sign stays unsigned and is never installed by Shale.
- No new dependency; `ring` already verifies ed25519. `ssh-keygen` is needed only by the owner (and
  by the one test that makes a real signature, which is skipped where it is missing).
- Past releases (no `.sig`) are not offered for self-install; they can be signed later with
  `scripts/sign-release.sh vX.Y.Z` if the owner wants them to be.
- The first release after this change is the first one that needs signing to be auto-installed; an
  older Shale (#421 without this) still installs on the checksum alone.

## The steps at each release

1. `scripts/make-release.sh …` as before; Forgejo CI passes; GitHub builds and publishes.
2. When the release page is up: `scripts/sign-release.sh vX.Y.Z` (add the key to `ssh-agent` first to
   type the passphrase once; `--dry-run` first if in doubt).
3. Check with `scripts/sign-release.sh --dry-run vX.Y.Z` again: it says "nothing to sign".
