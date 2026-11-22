### Fixed

- For developers: the storage check NET-001 now tests the file lock from a second process with Shale's own test binary instead of `flock(1)`, which macOS does not have, so the test suite passes on macOS again (#470).
