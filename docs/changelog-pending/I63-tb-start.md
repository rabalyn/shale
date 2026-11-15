### Internal

- **Tests for the Thunderbird reader, the start dialog and starting up.** A generator builds
  throwaway Thunderbird profiles (accounts, mbox and maildir folders, nested and non-ASCII folder
  names, indexes, deleted and large messages, broken files) for the tests of the import from
  Thunderbird (IMP-018). The start dialog (open or create a vault, recent vaults) is driven
  headlessly (VAULT-021, VAULT-022), the single-instance check and the command line are tested
  (DATA-012, DEV-013). No change in behaviour.
