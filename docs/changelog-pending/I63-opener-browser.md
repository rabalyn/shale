### Fixed

- **A mail attachment that Shale refuses to open (a program, a script, a `.desktop` file) no
  longer leaves its copy in Shale's temporary folder.** The copy was made before the check and
  stayed behind when the check said no (SEC-006).

### Changed

- Internal: tests now run the real opener against a recording fake `xdg-open`, and the OAuth2
  and Feeds sign-ins against a pretend browser (approve, deny, wrong state, never answers) and a
  pretend Mastodon and Bluesky server (SEC-006, MAIL-080 … MAIL-083, SOC-001 … SOC-004).
