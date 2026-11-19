//! The dev profile's debug info (BUILD-027): line tables only, for the program and its dependencies.

use std::fs;
use std::path::PathBuf;

/// The text of one `[section]` of `Cargo.toml` (up to the next section header).
fn section(text: &str, header: &str) -> String {
    let start = text.find(&format!("\n{header}\n")).unwrap_or_else(|| panic!("no section {header}")) + 1;
    let rest = &text[start + header.len() + 1..];
    let end = rest.find("\n[").unwrap_or(rest.len());
    rest[..end].to_string()
}

// spec: BUILD-027
#[test]
fn build_027_dev_profile_and_its_dependencies_keep_line_tables_only() {
    let text = fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).unwrap();
    for header in ["[profile.dev]", "[profile.dev.package.\"*\"]"] {
        let s = section(&text, header);
        assert!(s.lines().any(|l| l.trim() == "debug = \"line-tables-only\""), "{header} lacks debug = \"line-tables-only\"");
        assert!(s.contains("opt-level"), "{header} lost its opt-level");
    }
}
