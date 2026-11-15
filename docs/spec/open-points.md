# Open points and SHOULD BE work list

Questions that are not decided yet, and the SHOULD BE rules in the order the 2026-11-14 review
suggests doing them. When a point is decided it becomes an ADR or a spec rule and is struck out
here.

## Suggested order of SHOULD BE work

| # | Spec IDs | What | Why first | ADR |
|---|---|---|---|---|
| 1 | ~~DATA-004~~ | ~~Atomic writes everywhere~~ (done, DATA-004 and DATA-010) | A crash mid-write is the only failure that destroys a whole note | [0003](../decisions/0003-data-safety.md) |
| 2 | ~~DATA-003, DATA-002~~ | ~~Conflict copy instead of silently overwriting outside changes; reload drawings too~~ (done) | Silent lost updates with Syncthing / git / a second editor | 0003 |
| 3 | ~~DATA-008, DATA-009~~ | ~~Unreadable notes read-only; never swallow write errors~~ (done) | First keystroke can destroy a non-UTF-8 file | 0003 |
| 4 | ~~VAULT-018~~ | One checked vault path type at every user input | Rename can move a note out of the vault | [0004](../decisions/0004-link-resolution-and-paths.md) |
| 5 | ~~VAULT-016, VAULT-013~~ | One link rewrite routine by resolution (folder rename included) | Rename corrupts path links and rewrites wrong links | 0004 |
| 6 | ~~LINK-010, LINK-008~~ | Explicit resolution rules, no silent name fallback, show ambiguity | Blue links that open the wrong note | 0004 |
| ~~7~~ | CFG-004…CFG-008, SYNC-* | ~~Config version, keep broken file, per-vault state into `.shale/`, integrated git sync~~ Done 2026-11-14 | One parse error wipes calendars; vaults behave the same on every machine | [0005](../decisions/0005-where-state-lives.md), [0008](../decisions/0008-git-sync.md) |
| 8 | ~~JOB-002~~ | ~~Panic-safe background jobs~~ (done) | Endless spinners | [0006](../decisions/0006-engineering-baseline.md) |
| 9 | ~~SEC-006~~ | ~~Allowlist for `xdg-open`~~ (done) | Server- and mail-supplied URLs/files opened unchecked | 0006 |
| 10 | ~~BUILD-004~~, BUILD-006 | ~~CI on PRs, clippy/fmt gates~~ (done), drop vendor/ | Zero-warnings rule enforced, security updates possible | 0006 |
| 11 | PERF-001, PERF-002 | Cache derived data, fix O(n²) rescan | Big vaults | 0006 |
| 12 | — | Docs restructure | Stop drift | [0002](../decisions/0002-documentation-structure.md) |

## Open questions (not decided)

| ID | Question | Notes |
|---|---|---|
| OPEN-001 | Accept ADRs 0002–0006 as a whole, or one by one? | All are *Proposed*. |
| ~~OPEN-002~~ | ~~Conflict handling: conflict copy (proposed) or a merge view later?~~ | Decided 2026-11-14 by the owner's acceptance of [ADR-0003](../decisions/0003-data-safety.md): a conflict copy (DATA-003). A merge view could still build on it later. |
| ~~OPEN-003~~ | ~~Offer "fix this link" for red path links whose name exists elsewhere (after LINK-010 turns them red)?~~ | Decided 2026-11-14: links are fixed automatically, without asking, whenever the target is unambiguous; "Fix link" is offered only where it is not (LINK-012). |
| OPEN-004 | Should the media folder (`z_media/<YYYY>/<MM>/`) become a vault setting? | Today it is fixed; Obsidian users may expect their attachment folder. |
| OPEN-005 | Symbolic links in vaults: follow or show? | VAULT-007/008. Following needs loop and escape guards. |
| OPEN-006 | inotify file watching instead of 3 s polling? | Narrows the conflict window and saves work on big vaults; not a replacement for DATA-003. |
| OPEN-007 | Platform policy: Linux only, or keep the macOS/Windows code paths alive (and test them)? | `Cargo.toml` has macOS deps; `lists.rs` has Windows code; CI is Linux only. |
| OPEN-008 | Trash: restore from within Shale, and empty it after N days? | VAULT-019 records the origin first. |
| ~~OPEN-009~~ | ~~`.excalidraw.md` (Obsidian Excalidraw plugin) files: support or document as unsupported?~~ | Decided 2026-11-14: supported (DRAW-007, DRAW-008). Links and pictures inside them: OPEN-014. |
| ~~OPEN-014~~ | ~~Count `[[links]]` in drawings' texts as links, and show pictures embedded in plugin drawings?~~ | Decided 2026-11-14: yes, canvases included; canvases are still never written, so a rename names the canvases left behind (DRAW-016, DRAW-017, DRAW-018). |
| OPEN-010 | Calendar: moving events by dragging; showing a reminder when it is due (reminders are stored and edited, CAL-013, but Shale does not ring) — which first? | Backlog from IMPLEMENTATION.md. Single-occurrence edits (CAL-007) and editing reminders were done 2026-11-15. |
| OPEN-011 | Module restructuring (feature folders, splitting `ShaleApp`): do it along the way (proposed) or as one dedicated change? | ADR-0006 §6. |
| OPEN-012 | Add a test that checks spec IDs against test names (every ID used or marked `untested`)? | Prevents spec drift ([ADR-0001](../decisions/0001-spec-driven-development.md) §5). |
| OPEN-013 | Rename/move rewrites links in notes on disk while an open, dirty buffer of the same note exists — which wins? | Related to DATA-003; needs a test either way. |
| ~~OPEN-015~~ | ~~Mail: support OAuth2 in the first phase or later?~~ | Decided 2026-11-14: phase 1 ([ADR-0007](../decisions/0007-mail-client.md) §7). |
| ~~OPEN-016~~ | ~~Mail: big attachments with the message or on demand?~~ | Decided 2026-11-14: with the message. |
| ~~OPEN-017~~ | ~~Mail: index decrypted mails?~~ | Decided 2026-11-14: yes, after typed consent (ADR-0007 §6). |
| OPEN-018 | Release signing: sign the release tarball (and its `.sha256`) so a download can be checked against a key, not only against damage. Which tool (minisign, `gpg --detach-sign`, Sigstore/cosign), where the key lives, and whether the GitHub build or a local step signs? | To do (owner, 2026-11-15). Today BUILD-011 checksums only; a compromised release page could replace both files. |
| OPEN-019 | Wipe secrets from memory: passwords, OAuth2 refresh/access tokens and client secrets are plain `String`s that stay in freed memory and can end up in swap or core dumps. Use `zeroize`/`secrecy` (or own wrappers) for secrets from the dialog to the keyring call, and see which copies egui text fields keep. | To do (owner, 2026-11-15). Related: SEC-007, SEC-009, SEC-012, MAIL-021. |
| OPEN-020 | A system keyring on macOS (Keychain) and Windows (Credential Manager): `secrets.rs` talks only to `secret-tool` (libsecret), so on other systems secrets fall back to the owner-only settings file. | To do (owner, 2026-11-15). Depends on OPEN-007 (platform policy); related: SEC-003, SEC-011, MAIL-021. |

## Backlog carried over from IMPLEMENTATION.md (still open after checking)

Editor: blockquote continuation, indent width setting, live preview hiding syntax, tables,
auto-pairing brackets, find and replace in a note, spell check, paste images. Links: block
links (`#^id`), heading scroll in reading view, hover previews. Tasks: priorities, recurring
tasks, grouping. Graph: colour groups, filters, Barnes–Hut. Drawings: SVG/PDF pictures, frames, groups,
elbow arrows, labels on arrows, rotation handles, PNG/SVG export, copy/paste between drawings.
Other: command palette, daily notes, templates, light theme, split panes, packaging (.desktop,
Flatpak), native folder picker.
