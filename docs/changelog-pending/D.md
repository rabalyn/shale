### Fixed

- An autosave could land in the middle of a git sync's merge: the note was written, git then overwrote it with the merged version, and the text you had just typed was gone without a conflict copy. The sync and the window now take turns on one lock: the autosave skips a moment while git changes files, and every other save (save all, rename, move, trash, checkbox, fix link) waits for it for up to 30 seconds; if the sync is still busy after that, nothing is saved, your edits stay in the open note and a message says so. (SYNC-013)

### Added

- "Restore last deleted" in the right-click menu of the empty space of the file list puts the item you moved to the trash last back at its original place. It never replaces anything: if the place is taken, the item stays in the trash and a message says so. (VAULT-020)

### Changed

- The vault trash keeps where an item came from: it goes to `.trash/<date>/<its path in the vault>`, so two notes of the same name from different folders no longer get mixed up. A name already taken that day gets ` (2)`, ` (3)` … before the extension; nothing in the trash is ever replaced. Entries directly in `.trash/` from earlier versions stay where they are. (VAULT-017)
