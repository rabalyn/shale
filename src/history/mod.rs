//! The file history panel and the comparison view (HIST-001…HIST-018, rabalyn/shale#472).
//!
//! - [`diff`]: the line, word and block diffs and the patch text; pure.
//! - [`labels`]: days, times, sources in plain words and the size of a change; pure.
//! - [`place`]: margin column or sidebar panel; pure.
//!
//! The reading of the history and the actions (restore, copy) are in `app/history.rs`.

pub mod diff;
pub mod labels;
pub mod place;
