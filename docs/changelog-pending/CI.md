### Changed

- **Builds: day-to-day CI moved to a self-hosted Forgejo; GitHub builds only releases.** Every
  push and pull request is now formatted, linted, built, tested, packaged with a checked
  `.sha256` file and audited (`cargo audit`, also weekly) on the development server. GitHub runs
  only for version tags: it builds and tests the tag again and publishes the release with its
  `.sha256` file. The rolling "latest" pre-release of `main` is no longer updated; to try an
  unreleased version, build it from source. `make-release.sh` pushes the commit and the tag to
  both servers (BUILD-002, BUILD-009 … BUILD-012).
