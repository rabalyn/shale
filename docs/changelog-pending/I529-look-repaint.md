### Fixed

- For developers: the file look-up test waits for the window's repaint after a worker answer instead of checking it at once, so it no longer fails now and then under load (#529).
