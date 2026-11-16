//! Calendars: local `.ics` files and CalDAV servers (store, sync, iCalendar parsing, the month, week and day views and the event dialog).

pub(crate) mod dav;
#[cfg(test)]
mod dav_status_tests;
pub(crate) mod dialog;
#[cfg(test)]
mod env_tests;
pub(crate) mod ics;
pub(crate) mod store;
#[cfg(test)]
mod store_failure_tests;
pub(crate) mod time;
mod tzrule;
pub(crate) mod ui;
