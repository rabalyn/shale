### Fixed

- **Calendar: a CalDAV save or delete without a version tag no longer overwrites a change made on
  the server.** Shale first fetches the event and compares it with what it read; if it differs, the
  save or delete is refused with "the event was changed on the server meanwhile" (CAL-016).

### Changed

- **Calendar: "Delete event" asks first and keeps a copy.** The event popup shows "Delete “Title”?"
  (or "… with all its repeats?" / "Delete only this date …") with Delete and Keep. Before an
  event or one date of it goes, the whole event is written to `calendar-trash/<date>/` in the data
  folder; if that copy cannot be written, nothing is deleted. The message says where the copy is
  (CAL-011, CAL-017).
