//! How the code is laid out (DEV-008, ADR-0006 §6): a folder per feature with a parent file, and
//! `ShaleApp` owning sub-states.

use std::fs;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The folders of `src/` that hold no code: data the program embeds.
const DATA_FOLDERS: [&str; 1] = ["testdata"];

// spec: DEV-008
#[test]
fn dev_008_every_feature_folder_has_a_parent_file_and_main_declares_only_top_level_modules() {
    let src = root().join("src");
    let main = fs::read_to_string(src.join("main.rs")).unwrap();
    let mut folders = 0;
    for entry in fs::read_dir(&src).unwrap().flatten().filter(|e| e.path().is_dir()) {
        let name = entry.file_name().to_string_lossy().into_owned();
        if DATA_FOLDERS.contains(&name.as_str()) {
            continue;
        }
        folders += 1;
        assert!(src.join(format!("{name}.rs")).is_file() || src.join(&name).join("mod.rs").is_file(), "src/{name}/ has no parent file src/{name}.rs (or {name}/mod.rs)");
        assert!(main.lines().any(|l| l.trim_end() == format!("mod {name};") || l.trim_end() == format!("pub mod {name};")), "main.rs does not declare the module {name}");
    }
    assert!(folders >= 15, "the feature folders are found ({folders})");
    for feature in ["calendar", "mail", "drawing", "import", "pdf", "app", "social", "data_usage", "secrets", "gitsync"] {
        assert!(src.join(feature).is_dir(), "the feature folder {feature}/ the row names");
    }
    // main.rs declares top-level modules only: every `mod x;` is a file or folder next to it
    for line in main.lines().filter(|l| (l.starts_with("mod ") || l.starts_with("pub mod ")) && l.ends_with(';')) {
        let name = line.trim_start_matches("pub ").trim_start_matches("mod ").trim_end_matches(';').trim();
        assert!(src.join(format!("{name}.rs")).is_file() || src.join(name).join("mod.rs").is_file(), "main.rs declares {name}, which is not in src/");
    }
}

// spec: DEV-008
#[test]
fn dev_008_the_app_owns_its_sub_states_as_fields() {
    let text = fs::read_to_string(root().join("src/app.rs")).unwrap();
    let body = text.split("pub struct ShaleApp {").nth(1).and_then(|s| s.split("\n}\n").next()).expect("the struct ShaleApp");
    for (field, kind) in [
        ("drag", "DragState"),
        ("cal", "CalendarUi"),
        ("tag_learner", "TagLearner"),
        ("script", "ScriptRun"),
        ("tabbar", "TabBar"),
        ("search", "SearchBox"),
        ("tree", "FileTree"),
        ("graphs", "Graphs"),
        ("shot", "ScreenshotRun"),
        ("recovery", "RecoveryState"),
        ("busy", "Busy"),
        ("side", "SideState"),
    ] {
        let declared = body
            .lines()
            .map(str::trim_start)
            .filter_map(|l| l.strip_prefix("pub(crate) ").or_else(|| l.strip_prefix("pub ")).or(Some(l)))
            .any(|l| l.strip_prefix(field).and_then(|r| r.strip_prefix(": ")).is_some_and(|ty| ty.trim_end_matches(',').ends_with(kind)));
        assert!(declared, "ShaleApp has no field `{field}: …{kind}`");
    }
    assert!(body.lines().any(|l| l.trim_start().starts_with("frames: ") || l.trim_start().starts_with("pub frames: ")), "the frame counter stays loose");
}
