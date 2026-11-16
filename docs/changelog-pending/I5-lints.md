### Changed

- **Internal: the last two global clippy allows are gone.** `type_complexity` and `too_many_arguments` are
  linted like everything else; the 22 places that tripped them now use named type aliases and small
  parameter structs. No change in behaviour (DEV-002).
