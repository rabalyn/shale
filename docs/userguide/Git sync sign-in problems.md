---
features:
  - "Git sync"
---
When a sync cannot sign in to the server, Shale says why in plain words, what to do, and, for the cases below, a link **How to fix this** that opens this note at the right section. Back to [[Git sync]] and [[Turn on sync]].

Shale signs in in one of three ways, and it tests all three against real servers: **HTTPS with a token**, **SSH with a key that has no passphrase** (what Shale makes for you), and **SSH with a key of your own that has a passphrase**, which your **ssh-agent** unlocks. Shale never asks for a passphrase and never keeps one; an agent holds the unlocked key for you. Anything else that goes wrong is shown with git's own message.

# The token was not accepted

The server answered "authentication failed". Either the user name or the token is wrong, the token has expired, or it may not use repositories.

1. Open the token page of your server (the sign-in step of **Turn on sync** links to it: GitHub *Settings → Developer settings → Personal access tokens*, GitLab *Preferences → Access tokens*, Codeberg, Forgejo and Gitea *Settings → Applications*).
2. Make a new token that may read and write repositories, and copy it.
3. In Settings → Sync press **Turn on sync…** again, choose the same server, and paste the token at the sign-in step. Shale keeps it in your system's keyring and replaces the old one.

Without a keyring (some Linux setups) Shale cannot keep a token: use an SSH key instead.

# The server wants a token, not a password

Your server refused a password. GitHub no longer takes passwords for git, and a GitLab, Forgejo or Gitea account with two-factor login (2FA) can only sign in to git with a **personal access token**. The password of your account is never what Shale needs.

1. Make a token as in [[#The token was not accepted]].
2. Enter the token in the sign-in step, not your password. If an older sync of this vault still has your password in a git credential helper, remove it there (on Windows: *Credential Manager → Windows Credentials*, the entry of the server; on macOS: the Keychain Access entry of the server), so that git does not offer it first.

# The server does not know your SSH key

The server answered "Permission denied (publickey)": it does not have the public half of the key Shale used.

1. In Turn on sync → Sign in → SSH key press **Copy** next to the key (the file ending in `.pub` holds the same line).
2. Add it to your account on the server (GitHub *Settings → SSH and GPG keys*, GitLab *Preferences → SSH Keys*, Forgejo, Gitea and Codeberg *Settings → SSH / GPG Keys*).
3. Try again.

If you use a key of your own, check that it is the one Shale uses: the sign-in step shows which one is chosen.

# The key is locked: add it to the ssh-agent

Your key is protected by a passphrase, and your ssh-agent is running but does not hold the key. Shale cannot type a passphrase for you (it has no field for one, on purpose), so the key has to be unlocked once per login in your agent.

**Linux.** Open a terminal and run `ssh-add ~/.ssh/id_ed25519` (use the file name of your key), then type the passphrase. Most desktops (GNOME Keyring, KDE Wallet with `ksshaskpass`) start an agent for you and can remember the passphrase, so that this is needed only once. KDE: *System Settings → Startup and Shutdown → Autostart* can run `ssh-add` at login.

**macOS.** Run `ssh-add --apple-use-keychain ~/.ssh/id_ed25519` once. To load it at every login, add this to `~/.ssh/config`:

```
Host *
  AddKeysToAgent yes
  UseKeychain yes
```

**Windows.** Start the **OpenSSH Authentication Agent** service (*Services → OpenSSH Authentication Agent → Startup type: Automatic*, then Start), then run `ssh-add $env:USERPROFILE\.ssh\id_ed25519` in PowerShell. Read [[#No ssh-agent is running]] too: the ssh that comes with Git for Windows does not use that service by default.

Then press **Try again** or **Sync now**. Shale and git use the same agent as your terminal (the `SSH_AUTH_SOCK` variable); if you start Shale from a launcher that does not know it, start Shale from a terminal where `ssh-add -l` lists your key, or log out and in again after the agent started.

# No ssh-agent is running

Your key is protected by a passphrase, and Shale found no ssh-agent that answers. Without an agent the key cannot be unlocked (Shale does not ask for the passphrase).

**Linux.** Most desktops start an agent at login; if yours does not, add `eval "$(ssh-agent -s)"` to your shell's start file, or turn on your distribution's user service for ssh-agent (`systemctl --user enable --now ssh-agent` on systems that have the unit) and make sure `SSH_AUTH_SOCK` names its socket. Then add the key as in [[#The key is locked: add it to the ssh-agent]].

**macOS.** The system starts an agent for every user; run `ssh-add -l` in a terminal. If it says it cannot connect, log out and in again, then add the key.

**Windows.** Two sshs are around: the one of Windows (`C:\Windows\System32\OpenSSH\ssh.exe`), which talks to the *OpenSSH Authentication Agent* service, and the one that Git for Windows brings, which does not. If git uses the second one, the service's agent is invisible to it. Make git use the first: run `git config --global core.sshCommand "C:/Windows/System32/OpenSSH/ssh.exe"` in a terminal (a vault that signs in with a key Shale made has its own setting and is not changed by this), start the service as in [[#The key is locked: add it to the ssh-agent]] and add the key.

Or avoid the passphrase problem: make a key with Shale (Turn on sync → Sign in → SSH key → **Make a new key**), which has none, or sign in with a token.

# The server's host key changed

SSH remembers the key of each server in `known_hosts` and stops when the server shows another one ("REMOTE HOST IDENTIFICATION HAS CHANGED" or "Host key verification failed"). Shale stops too, and does not go on by itself, because that is also what an attack on your connection looks like.

- **You did not expect a change:** do not go on. Ask whoever runs the server, or compare the fingerprint with the one the provider publishes (GitHub, GitLab and Codeberg list theirs in their documentation).
- **The server was set up again or moved:** remove the old entry and connect once to trust the new key. In a terminal run `ssh-keygen -R server.example.org` (for a port other than 22: `ssh-keygen -R "[server.example.org]:2222"`), then `ssh git@server.example.org` once and answer **yes** after you checked the fingerprint. On Windows the file is `%USERPROFILE%\.ssh\known_hosts`.
- **"No host key is known":** the server is new to this computer and Shale's background syncs may not ask. Run the `ssh git@server.example.org` once as above (the test sync of **Turn on sync** trusts a new server once by itself).

# A login page is in front of the server

Git got a web page (HTML) or a redirect to a sign-in page where it expected git. A proxy or single sign-on (a company gateway, a "zero trust" service, a captive portal of a hotel or café network) answers before the git server does, and it wants a browser sign-in that git cannot do.

1. **A captive portal** (hotel, café): open a browser, sign in to the network, then try again.
2. **A proxy in front of your own server:** let the git paths through it with token authentication instead of the single sign-on. For Forgejo, Gitea, GitLab and GitHub Enterprise these are `/<owner>/<repo>.git/…` and `/api/…`; ask whoever runs the proxy for an exception that checks the token, not the browser session.
3. **Or use SSH:** an SSH address (`git@server:owner/repo.git`) does not go through the web proxy at all. Turn on sync again, choose **SSH key**, and add the key on the server.

A server that answers a web page for a git address also looks like an empty repository when Shale only reads it. The upload is where this shows.
