//! The committed dataflow diagrams (`docs/dataflow/*.md`, DEV-026) name only steps that exist in
//! the code. The diagrams themselves are checked against a real run by the mail-env test.

use std::fs;
use std::path::{Path, PathBuf};

fn sources(dir: &Path, out: &mut Vec<String>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path: PathBuf = entry.unwrap().path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(fs::read_to_string(path).unwrap());
        }
    }
}

/// The step names a block shows: the label of an arrow, the name of a `loop` or `opt` block (but
/// not the generic `repeated` and `sometimes`) and the text of a `Note over` line.
fn steps_of(block: &str) -> Vec<&str> {
    block
        .lines()
        .map(str::trim)
        .filter_map(|l| {
            if let Some((_, rest)) = l.split_once("->>") {
                return rest.split_once(": ").map(|(_, label)| label.trim());
            }
            if let Some(rest) = l.strip_prefix("Note over ") {
                return rest.split_once(": ").map(|(_, label)| label.trim());
            }
            l.strip_prefix("loop ").or_else(|| l.strip_prefix("opt ")).map(str::trim).filter(|name| !matches!(*name, "repeated" | "sometimes"))
        })
        .collect()
}

// spec: DEV-026
#[test]
fn dev_026_steps_come_from_arrows_blocks_and_notes() {
    let block = "```mermaid\nsequenceDiagram\n    participant sync\n    Note over sync: a\n    loop b\n        opt c\n            sync->>imap: d\n        end\n    end\n    loop repeated\n        opt sometimes\n            sync->>store: e\n        end\n    end\n```\n";
    assert_eq!(steps_of(block), ["a", "b", "c", "d", "e"]);
}

// spec: DEV-026
#[test]
fn dev_026_every_step_of_a_dataflow_diagram_is_a_span_in_the_code() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut code = Vec::new();
    sources(&root.join("src"), &mut code);
    let code = code.join("\n");
    let mut diagrams = 0;
    for entry in fs::read_dir(root.join("docs/dataflow")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "md") {
            continue;
        }
        let doc = fs::read_to_string(&path).unwrap();
        let mut rest = doc.as_str();
        while let Some(at) = rest.find("<!-- dataflow:begin ") {
            let after = &rest[at + "<!-- dataflow:begin ".len()..];
            let (flow, tail) = after.split_once(" -->").unwrap_or_else(|| panic!("{}: a begin marker is not closed", path.display()));
            let end = format!("<!-- dataflow:end {flow} -->");
            let (block, next) = tail.split_once(&end).unwrap_or_else(|| panic!("{}: no end marker for {flow}", path.display()));
            let labels = steps_of(block);
            assert!(!labels.is_empty(), "{}: the diagram of {flow} has no steps", path.display());
            for label in labels {
                let name = format!("\"flow.{flow}.{label}\"");
                assert!(code.contains(&name), "{}: the step {label} of {flow} is not a span ({name}) in src/", path.display());
            }
            diagrams += 1;
            rest = next;
        }
    }
    assert!(diagrams > 0, "docs/dataflow has a diagram");
}
