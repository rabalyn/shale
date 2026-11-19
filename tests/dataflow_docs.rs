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

/// A label of the diagram of `flow` is a span: `flow.<flow>.<label>` for a step of its own,
/// `flow.<label>` for a step of another flow (`other.step` is `flow.other.step`, a bare `other`
/// is the root `flow.other`).
fn is_span(code: &str, flow: &str, label: &str) -> bool {
    code.contains(&format!("\"flow.{flow}.{label}\"")) || code.contains(&format!("\"flow.{label}\""))
}

// spec: DEV-026
#[test]
fn dev_026_a_label_is_a_step_of_the_flow_or_a_step_or_root_of_another_flow() {
    let code = "info_span!(\"flow.mine.own\") info_span!(\"flow.other.step\") info_span!(\"flow.nested\")";
    assert!(is_span(code, "mine", "own"));
    assert!(is_span(code, "mine", "other.step"));
    assert!(is_span(code, "mine", "nested"));
    assert!(!is_span(code, "mine", "step"));
    assert!(!is_span(code, "mine", "other.missing"));
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
                assert!(is_span(&code, flow, label), "{}: the step {label} of {flow} is not a span (\"flow.{flow}.{label}\" or \"flow.{label}\") in src/", path.display());
            }
            diagrams += 1;
            rest = next;
        }
    }
    assert!(diagrams > 0, "docs/dataflow has a diagram");
}

/// The rows of the index table in `docs/dataflow/README.md`: the lines that start with `|`, minus
/// the header and the separator, each as its trimmed cells.
fn index_rows(readme: &str) -> Vec<Vec<&str>> {
    readme.lines().filter(|l| l.starts_with('|')).skip(2).map(|l| l.trim_matches('|').split('|').map(str::trim).collect()).collect()
}

// spec: DEV-027
#[test]
fn dev_027_index_rows_are_the_table_lines_after_the_header() {
    let readme = "text\n| Flow | Doc |\n| --- | --- |\n| a | [a.md](a.md) |\n| b | [b.md](b.md) |\n\nmore\n";
    assert_eq!(index_rows(readme), [["a", "[a.md](a.md)"], ["b", "[b.md](b.md)"]]);
}

// spec: DEV-027
#[test]
fn dev_027_every_dataflow_block_is_in_the_index_and_every_indexed_flow_has_its_block() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = root.join("docs/dataflow");
    let mut blocks = Vec::new(); // (flow, file name)
    for entry in fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if name == "README.md" || path.extension().is_none_or(|e| e != "md") {
            continue;
        }
        let doc = fs::read_to_string(&path).unwrap();
        for part in doc.split("<!-- dataflow:begin ").skip(1) {
            let flow = part.split_once(" -->").unwrap_or_else(|| panic!("{name}: a begin marker is not closed")).0;
            blocks.push((flow.to_string(), name.clone()));
        }
    }
    let readme = fs::read_to_string(dir.join("README.md")).unwrap();
    let rows = index_rows(&readme);
    let mut code = Vec::new();
    sources(&root.join("src"), &mut code);
    sources(&root.join("tests"), &mut code);
    let code = code.join("\n");
    for row in &rows {
        assert_eq!(row.len(), 4, "README.md: a row needs Flow, Doc, Driving test and Drift checked by: {row:?}");
        let flow = row[0];
        let link = row[1].split_once("](").and_then(|(_, r)| r.strip_suffix(')')).unwrap_or_else(|| panic!("README.md: the Doc of {flow} is not a markdown link"));
        assert!(blocks.iter().any(|(f, file)| f == flow && file == link), "README.md: {flow} is listed with {link}, which has no dataflow block for it");
        let test = row[2].trim_matches('`').rsplit("::").next().unwrap();
        assert!(code.contains(&format!("fn {test}(")), "README.md: the driving test {test} of {flow} is not a fn in src/ or tests/");
        assert!(!row[3].is_empty(), "README.md: {flow} says nothing about how its drift is checked");
    }
    for (flow, file) in &blocks {
        assert!(rows.iter().any(|r| r[0] == flow), "{file}: the block {flow} is not in the table of docs/dataflow/README.md");
    }
}
