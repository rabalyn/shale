//! What the report programs of `examples/` share: `loc_report.rs` (DEV-044) and `size_report.rs`
//! (BUILD-045) each include this file with `#[path = "report_common/mod.rs"] mod report_common;`.
//! Both use every function, so neither has dead code.

use std::fs;
use std::path::Path;

/// A step of 1, 2 or 5 times a power of ten and the top of the y axis (a multiple of the step, at
/// least `peak`), so that there are at most five intervals.
pub fn nice_scale(peak: usize) -> (usize, usize) {
    let raw = peak.div_ceil(5).max(1);
    let mut step = 1;
    while step < raw {
        step = match step.to_string().as_bytes()[0] {
            b'1' => step * 2,
            b'2' => step / 2 * 5,
            _ => step * 2,
        };
    }
    (step, (peak.div_ceil(step) * step).max(step))
}

/// A count with a space between the thousands: `98 754`.
pub fn spaced(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(' ');
        }
        out.push(ch);
    }
    out
}

pub fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

/// Writes `text` unless the file holds it already.
pub fn write_if_changed(path: &Path, text: &str) -> Result<(), String> {
    if fs::read_to_string(path).is_ok_and(|old| old == text) {
        return Ok(());
    }
    fs::write(path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))
}
