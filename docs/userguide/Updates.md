---
features:
  - "Updates"
---
![[settings-updates.png]]

Settings → **Updates** decides what Shale does when a new release is out on GitHub
(github.com/rabalyn/shale). Back to [[Start here]]; every key is in [[Settings reference]] (`updates`).

## The four choices

Until you choose, Shale looks like **Notify**, and the first time it finds an update it asks.

- **Off**: Shale never asks the network about updates and shows nothing about them. The same
  happens when the environment has `SHALE_UPDATES=off` (for packagers and scripts); Settings says so.
- **Notify**: a notice in the corner says "Shale X.Y.Z is available." with **What's new** (the release
  notes), **Download**, **Install**, **Skip this version** and **Later**. Skip hides that version only;
  the next release is announced again. Later hides the notice until the next check.
- **Download**: Shale downloads the new version in the background and checks it, then the notice
  says it is ready: **Install now**, or **At next start** (the next start installs it by itself).
- **Install**: Shale downloads, checks and installs it, then the notice says "Updated to Shale X.Y.Z."
  with **Restart now**. Restart saves everything the way quitting does, and starts the new version.
  **Later** just keeps working: the new version is used the next time you start Shale.

Shale checks a short while after it starts and then every 24 hours; **Check for updates now** in
Settings does not wait and says in words what it found ("You have the latest version, 0.6.6.", the
new version, or why it could not look).

## What is checked, and what is not

The release has a `.sha256` file next to each download. Shale compares the SHA-256 of what it
downloaded with it, and installs nothing when they differ (the download is deleted and the notice says
so). That shows the file arrived whole. It does **not** show who made it: the checksum comes from
the same release as the file. If that matters to you, choose **Notify** or **Off** and check the
release yourself.

## How Shale was installed decides what an update does

Settings → Updates shows how this copy was installed and, if it cannot update itself, why.

| Installed as | An update |
|---|---|
| Linux, run from the unpacked download | replaces the program and the other files of the folder in place; the old program stays as `shale.old` |
| Linux, `install.sh` (`~/.local`, or a `--prefix`) | runs the new `install.sh` again; the old program stays as `shale.old` next to it |
| Linux, from the package manager (AUR `shale-bin`, `/usr/bin`) | **never touched**: a notice says "Update Shale with your package manager (for example `yay -Syu shale-bin`)" |
| macOS, `Shale.app` | mounts the dmg, puts the new app next to the old one and swaps them, then opens it again. The build is signed ad hoc, so the Keychain may ask for access again |
| Windows, the unpacked zip | renames the running `shale.exe` to `shale.exe.old`, puts the new files in place and starts it again; the next start removes the old file |
| a development build (a `target` folder) | never touched; only a notice |

A folder you cannot write (for example `install.sh --prefix /usr/local` without root) turns Install into
Download: Shale downloads and checks the update and the notice shows the command to run, with
**Copy the command**.

## Going back

Where Shale keeps the old program it is `shale.old` next to `shale` (Linux, until the new version has run
for half a minute; then it is removed). To go back before that, close Shale and run
`mv shale.old shale` in the folder. After that, install the older release from its download.

The downloads wait in `~/.cache/shale/updates/<version>/` until they are installed. See also
[[Data usage and caches]].

#userguide #settings
