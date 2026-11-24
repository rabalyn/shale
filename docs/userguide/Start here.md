---
folder-note: true
title: Shale user guide
logo: z_media/shale-logo.png
subtitle: Notes, links, drawings, mail, calendar and feeds in one native app
---
Welcome. You are reading this guide **in Shale**: it is a vault, a plain folder of markdown
notes, and it uses the features it explains. Click a blue link, press Ctrl+E to switch between
editing and reading, open the graph from the ribbon and see how the notes connect.

![[How notes connect.excalidraw]]

*(That is a drawing, embedded with `![[How notes connect.excalidraw]]`. Double-click it in the
file list to edit it.)*

# Where to go

Writing and finding:

- [[Notes and links]]: wiki links, embeds, suggestions, moving and renaming.
- [[Editor]]: line numbers, Markdown preview, vim-style editing, lists.
- [[Reading view]]: the note without the markup.
- [[Search and pickers]]: find notes, text, mail and posts.
- [[Search syntax]]: what you can type in a search, for notes, mail and posts.
- [[Tabs]]: reorder, pin, come back later.
- [[Tags and people]]: tags, tag suggestions and contact notes.
- [[Sounds and videos]]: players for audio and video in your notes.

Organising:

- [[Tasks]]: every to-do of the vault in one list.
- [[Boards]]: Kanban boards, lanes made from queries, cards that are notes.
- [[Drawings]]: sketches, Obsidian drawings and canvases.
- [[Graph and backlinks]]: how your notes connect.
- [[Window layout]]: ribbon, sidebars and dialogs.
- [[Background work]]: spinners, Cancel and the indicator at the top right.
- [[Keyboard shortcuts]]: all keys, all changeable.

Your other data:

- [[Calendar]]: local and CalDAV calendars.
- [[Mail]]: IMAP, writing, signing and encrypting.
- [[Feeds]]: Mastodon and Bluesky.
- [[Import]]: mails, events and files become notes, and notes become Word files.
- [[Thread view]]: the mails of one conversation, in date order, and keys to step through them.
- [[PDF]]: export notes, read PDFs.
- [[Publishing a vault]]: your notes as a website, built with Hugo; [[Publishing to a server]]: how a server builds and serves it.

Keeping it safe and tidy:

- [[Turn on sync]]: keep your notes safe with a history on this computer, or sync them with a server.
- [[Git sync]]: the same vault on several computers.
- [[File history]]: earlier versions of a note, and getting a deleted note back.
- [[Vaults in cloud and network folders]]: Nextcloud, Dropbox, OneDrive, network drives: what is safe.
- [[Vault settings]]: what travels with a vault.
- [[Data usage and caches]]: what Shale keeps on disk.
- [[Updates]]: new releases: notify, download or install, and what is checked.
- [[Settings reference]]: every setting, its default, and where it lives.
- [[Fonts]]: the font and text size of the editor, dialogs, mail, calendar and feeds.
- [[Themes]]: light and dark colour themes, your own Alacritty theme files.

# Where this guide comes from

Press **F1** in Shale (or Settings → Help → **Open user guide**) to open it. The guide is built
into the program, which unpacks it once per version into its data folder (on Linux
`~/.local/share/shale/userguide-<version>/`; on macOS and Windows the "data" folder of the
system) and opens that copy as a vault. Your notes, tabs and edits stay in the copy: the next
version of Shale makes a new copy, and a copy that exists is never overwritten. The release
also has the guide as `shale-userguide-<version>.zip`, to unpack anywhere and open as a vault,
and as one PDF to print or read anywhere, `shale-userguide-<version>.pdf` (made from this vault
as a book).

# Installing on macOS

Open `shale-<version>-macos-arm64.dmg` (Apple silicon) and drag **Shale** onto the **Applications** link in the window. The first
start needs one extra step, because the app is not notarised by Apple (it is signed ad hoc): right-click Shale in Applications and
choose **Open**, then **Open** again; or try to open it once and choose **Open Anyway** in System Settings → Privacy & Security; or
run `xattr -dr com.apple.quarantine /Applications/Shale.app` in the Terminal. Because the signature changes with every version, the
Keychain may ask again for access to your saved passwords after an update. Shale finds programs from Homebrew (`git-lfs`, `ffmpeg`,
`gpg`, `pandoc`) even though Finder starts it with a short search path.

# Installing on Linux

The Linux download unpacks to a folder `shale/` with an `install.sh`. Run `./install.sh` in it to
put Shale in `~/.local/bin` with its app menu entry and icons, or `sudo ./install.sh --prefix /usr/local`
for all users; `./install.sh --uninstall` removes what it installed (your vaults and settings stay).
Running it again upgrades. If a library is missing, the script names it.

On Arch Linux, the package `shale-bin` installs the same download system-wide: build it from
`packaging/aur/shale-bin/` in the source with `makepkg -si`, or install it with `yay -S shale-bin` or
`paru -S shale-bin` once it is on the AUR.

# How to read this guide

Each note names the setting or key you need, and every picture was made by the app itself
(`docs/screenshots/regen.sh`), so it shows the current look; the short videos were recorded
by it too (`docs/screenshots/regen-clips.sh`). Things that are planned but not
built yet are not described here.

- [ ] Open the graph (ribbon) and find the note with the most links
- [ ] Press Ctrl+O and jump to [[Keyboard shortcuts]] by typing a few letters of its name
- [x] Open this guide as a vault

#userguide #start
