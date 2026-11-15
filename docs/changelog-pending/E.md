### Fixed

- The troubleshooting key recording no longer writes anything typed or pasted into any password
  field (social accounts, the mail password prompt, calendar and mail settings): every password
  field is made one way, and a test refuses a new one made differently. When Shale starts with the
  recording on, a notice says where it is written and how to switch it off.
- A CalDAV or social-account password that the system keyring refuses (for example the unlock
  was cancelled) is no longer written to the settings file in plain text. It is kept for this run,
  Shale says it was not saved, and a password already in the file from an earlier version stays there
  until the keyring takes it. Without any keyring nothing changes (the owner-only settings file).
- A renewed mail sign-in (OAuth) that could not be saved in the keyring is kept for this run, saved
  at the next renewal, and reported once instead of silently needing a new sign-in after a restart.
- GnuPG's temp folders are no longer made directly in the shared temp folder: they are made new
  inside a private folder, and an existing path is never adopted.

### Changed

- Git sync refuses a remote address with a password in it (use a credential helper or an SSH key)
  and `http://` or `git://` addresses unless the host is this computer. A password in an existing
  remote is shown as `***` in the settings and in error messages.
