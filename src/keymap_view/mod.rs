//! The keymap view (KEY-055…KEY-059, rabalyn/shale#710): the keyboard drawn with the actions on each
//! key, for a system and a layer of modifiers, coloured by the kind of action. It shows the live
//! bindings of the user, the fixed keys of the program and the keys the system reserves.
//!
//! * `model` and `tables`: pure data (no egui widgets), unit-tested; the two tables are in one place
//!   so that a later export can read them (rabalyn/shale#712).
//! * `layout`: the rows of keys of an ANSI and an ISO keyboard and the block of the navigation keys.
//! * `ui`: the widget, used by the Keybindings tab of the Settings (List | Keyboard) and by the
//!   Keymap tab of the main area.

mod host;
mod layout;
pub mod model;
pub mod tables;
mod ui;

#[cfg(test)]
mod model_tests;
#[cfg(test)]
mod ui_tests;

pub use ui::{show, State};
