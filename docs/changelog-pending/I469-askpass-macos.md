### Fixed

- For developers: the askpass helper's integration test, which fakes the Linux keyring (`secret-tool`), now runs only on Linux; on macOS and Windows the helper is covered by its unit tests, so the test suite passes on macOS again (#469).
