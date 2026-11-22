### Fixed

- For developers: the test suite now passes when run as root (as in the CI containers) and under parallel load in release builds. Flaky UI waits in the sync, managed-git and file-history tests were replaced by waiting for the background jobs to finish, two history tests no longer share a vault folder, and the move-out failure test no longer relies on an unreadable file (#469, #468, #471).
