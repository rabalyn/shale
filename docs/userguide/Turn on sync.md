---
features:
  - "Git sync"
---
**Turn on sync** is the short way to keep your notes safe: it asks what you need and sets it up. Back to [[Start here]]. What runs behind it is in [[Git sync]] and [[File history]].

Open Settings → Sync and press **Turn on sync…** (a vault that already syncs with a server does not need it; its remote is shown on the page).

![[wizard-choose.png]]

# Three ways to keep your notes safe

- **History on this device only.** Shale keeps earlier versions of your notes on this computer, in your data folder, outside the vault
  ([[File history]]). No server, no account, and nothing but a small ID file is added to your vault folder.
- **My folder is already synced by another app.** Nextcloud, Dropbox, OneDrive, iCloud, Syncthing or a network drive carries the files
  between your computers. Shale does not touch that: it keeps the history on each computer, outside the folder, and puts no `.git`
  folder into it. Folders of Nextcloud, Dropbox, OneDrive and Syncthing are safe for your notes ([[Vaults in cloud and network folders]]).
- **Sync with a server.** GitHub, GitLab, Codeberg, Forgejo, Gitea or any git address. Your notes go to the server and to your other
  computers; this vault gets a `.git` folder, as in [[Git sync]].

Without git on the computer the first step offers the install of [[Git sync]] first, and goes on by itself once git is there.

# Where is my vault?

Shale looks, in the background, whether the vault is on a network share (NFS, SMB, WebDAV, a Windows network drive) or in a folder
that Dropbox, Nextcloud, ownCloud, Syncthing, Google Drive, iCloud or OneDrive syncs. It only guesses, from the file system and from
marker files in the folder and its parents, so it never decides anything: it says what it found and what that means.

![[wizard-warning.png]]

A vault that already has a `.git` folder **inside** such a folder gets a stronger warning: sync apps copy the files of a repository
one by one and in any order, which can break it. If the vault has no server, **Move the repository out** copies the repository to a
private place on this computer, checks that the copy is the same (its branches and `git fsck`), renames it into place and only then
removes `.git` from the vault. A failure at any step leaves the vault as it was. A vault with a server is offered nothing: the server
keeps the notes safe, and the warning says what to do if the repository breaks (clone it again into a folder no app syncs).

# The server path

1. **The server.** Press GitHub, GitLab or Codeberg, or enter a host name (`git.example.org`), the address of a repository or an
   SSH address (`git@host:owner/repo.git`). For a server Shale does not know, say whether it is Forgejo or Gitea, GitHub or GitLab, or
   any git server. A folder on this computer or a network share (an absolute path, spaces in its name are fine) also works and needs no sign-in.

   ![[wizard-server.png]]

2. **Sign in.** Either with a **personal access token** or with an **SSH key**.

   ![[wizard-sign-in.png]]

   - *Token.* The button opens the page of the provider where you make one (GitHub, GitLab, Codeberg and any Forgejo or Gitea host).
     Paste it and press **Check the token**: where the provider has an API Shale asks who the token belongs to. The token is kept in the
     system keyring (the Secret Service of your desktop, the macOS Keychain or the Windows Credential Manager), never in the settings
     file, in `.git/config`, in the address or on a command line; git asks Shale for it when it syncs. Without a keyring Shale
     does not keep a token at all and recommends an SSH key.
   - *SSH key.* Shale lists the keys of `~/.ssh` and can **Make a new key** (ed25519, without a passphrase, `~/.ssh/shale_<server>`; it never
     overwrites a file). The wizard shows the public key with **Copy the public key** and opens the provider's page where you add it.
     The first test sync trusts a server you have not seen before and remembers it; a changed key is refused.

3. **The repository.** For Forgejo, Gitea and GitHub the wizard lists your repositories and can **Create a private repository**. For
   GitLab and other servers it links to the website where you make one and takes its address (`owner/name` or the full address).
4. **Test the sync.** The wizard first asks the server what its repository holds and says what will happen: an empty repository gets
   your notes; a repository with notes and an empty vault downloads them; with notes on both sides they are merged, nothing is deleted
   and a note that differs is kept in both versions (one as `Note (conflict …).md`). **Run the test sync** is one real sync. The result is
   in plain words: it worked; the user name or token was not accepted; no network to the server; no repository at that address; the SSH key
   was not accepted; the server's certificate cannot be checked. After a failure **Back** goes to the step that fixes it, and the remote
   stays set so that **Try again** works. **Finish** turns on "Sync this vault automatically".

**Cancel** and **Back** change nothing in the vault. Provider sign-in through a browser (OAuth or a device code) is not offered yet.

#userguide #git
