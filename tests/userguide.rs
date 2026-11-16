//! The user guide vault (`docs/userguide`, DEV-018) keeps up with the program: every feature
//! section of `docs/FEATURES.md` has a guide note, every setting is in the settings reference,
//! and the guide is a good vault itself (no red links, no missing pictures).

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn guide() -> PathBuf {
    root().join("docs/userguide")
}

/// Every file of the guide (folders that start with a dot are Shale's own and skipped).
fn guide_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')) {
            continue;
        }
        if path.is_dir() {
            guide_files(&path, out);
        } else {
            out.push(path);
        }
    }
}

fn notes() -> Vec<(PathBuf, String)> {
    let mut files = Vec::new();
    guide_files(&guide(), &mut files);
    files.into_iter().filter(|p| p.extension().is_some_and(|e| e == "md")).map(|p| (p.clone(), fs::read_to_string(p).unwrap())).collect()
}

/// The values of the `features:` list in the front matter of a note.
fn features_of(text: &str) -> Vec<String> {
    let Some(rest) = text.strip_prefix("---\n") else { return vec![] };
    let Some((front, _)) = rest.split_once("\n---\n") else { return vec![] };
    let mut out = vec![];
    let mut inside = false;
    for line in front.lines() {
        if line.starts_with("features:") {
            inside = true;
        } else if inside {
            match line.trim_start().strip_prefix("- ") {
                Some(v) => out.push(v.trim().trim_matches('"').to_string()),
                None => inside = false,
            }
        }
    }
    out
}

/// The `## ` headings of FEATURES.md outside code fences: the feature sections.
fn feature_sections(text: &str) -> Vec<String> {
    let mut fenced = false;
    let mut out = vec![];
    for line in text.lines() {
        if line.starts_with("```") {
            fenced = !fenced;
        } else if !fenced {
            if let Some(h) = line.strip_prefix("## ") {
                out.push(h.trim().to_string());
            }
        }
    }
    out
}

// spec: DEV-018
#[test]
fn dev_018_every_feature_section_of_the_features_doc_has_a_guide_note() {
    let sections = feature_sections(&fs::read_to_string(root().join("docs/FEATURES.md")).unwrap());
    assert!(sections.len() > 10 && sections.iter().any(|s| s == "Git sync"), "the sections of FEATURES.md are read");
    let covered: BTreeSet<String> = notes().iter().flat_map(|(_, t)| features_of(t)).collect();
    let missing: Vec<&String> = sections.iter().filter(|s| !covered.contains(*s)).collect();
    assert!(missing.is_empty(), "docs/FEATURES.md has sections without a note in docs/userguide (add the note, with the exact heading in its front matter `features:` list): {missing:?}");
    let unknown: Vec<&String> = covered.iter().filter(|c| !sections.contains(c)).collect();
    assert!(unknown.is_empty(), "a guide note claims a FEATURES.md section that does not exist (renamed?): {unknown:?}");
}

/// The fields (`pub name:`) of `pub struct <name>` in a source file.
fn fields_of(source: &str, name: &str) -> Vec<String> {
    let start = source.find(&format!("pub struct {name} {{")).unwrap_or_else(|| panic!("struct {name} not found"));
    let mut out = vec![];
    for line in source[start..].lines().skip(1).take_while(|l| *l != "}") {
        if let Some(f) = line.trim().strip_prefix("pub ").and_then(|l| l.split_once(':')).map(|(f, _)| f) {
            out.push(f.to_string());
        }
    }
    out
}

// spec: DEV-018
#[test]
fn dev_018_every_setting_is_in_the_settings_reference() {
    let reference = fs::read_to_string(guide().join("Settings reference.md")).unwrap();
    let mut missing = vec![];
    let mut seen = 0;
    for (file, structs) in [("src/config.rs", &["Config", "MailAccount", "CacheRetention", "SidebarCfg", "RightViewCfg"][..]), ("src/vault_state.rs", &["VaultSettings", "SyncSettings", "VaultState", "SavedTab"][..])] {
        let source = fs::read_to_string(root().join(file)).unwrap();
        for s in structs {
            for f in fields_of(&source, s) {
                // `other` keeps what a newer Shale wrote, `read_only` is a flag of the loaded file and `legacy_…` fields
                // only move old values into the vault (CFG-008): none of them is a setting
                if f == "other" || f == "read_only" || f.starts_with("legacy_") {
                    continue;
                }
                seen += 1;
                // the key alone, or as the end of a dotted path: `font_size`, `sync.enabled`, `caches.picture_days`, `mail_accounts[].host`
                if !reference.contains(&format!("`{f}`")) && !reference.contains(&format!(".{f}`")) {
                    missing.push(format!("{s}.{f} ({file})"));
                }
            }
        }
    }
    assert!(seen > 50, "the settings fields are read ({seen})");
    assert!(missing.is_empty(), "settings that are not in docs/userguide/Settings reference.md (write the key in backticks, its default and where it is changed): {missing:?}");
}

/// Text without its inline code spans (links in code are examples, not links).
fn without_code(line: &str) -> String {
    line.split('`').enumerate().filter(|(i, _)| i % 2 == 0).map(|(_, s)| s).collect::<Vec<_>>().join(" ")
}

/// The targets of the `[[…]]` links and `![[…]]` embeds of a note, outside code.
fn link_targets(text: &str) -> Vec<String> {
    let mut fenced = false;
    let mut out = vec![];
    for line in text.lines() {
        if line.starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        for part in without_code(line).split("[[").skip(1) {
            if let Some((inner, _)) = part.split_once("]]") {
                let target = inner.split(['|', '#']).next().unwrap_or("").trim();
                out.push(target.to_string());
            }
        }
    }
    out
}

// spec: DEV-018
#[test]
fn dev_018_the_guide_has_no_red_links_and_every_embedded_picture_exists() {
    let mut files = Vec::new();
    guide_files(&guide(), &mut files);
    // what a link by name finds: the file name, and for notes the name without `.md` (LINK-002), any case
    let names: BTreeSet<String> = files
        .iter()
        .flat_map(|p| {
            let n = p.file_name().unwrap().to_string_lossy().to_lowercase();
            let stem = n.strip_suffix(".md").map(str::to_string);
            [Some(n), stem]
        })
        .flatten()
        .collect();
    let mut red = vec![];
    for (path, text) in notes() {
        for t in link_targets(&text) {
            if !t.is_empty() && !t.contains('/') && !names.contains(&t.to_lowercase()) {
                red.push(format!("[[{t}]] in {}", path.strip_prefix(guide()).unwrap().display()));
            }
        }
    }
    assert!(red.is_empty(), "red links in the user guide (a note or picture is missing): {red:?}");
}

// spec: DEV-018, DEV-007
#[test]
fn dev_018_the_guide_is_a_vault_of_its_own_and_holds_the_screenshots() {
    assert!(guide().join("Start here.md").is_file(), "the start note");
    assert!(!guide().join(".git").exists(), "the guide is no git repository of its own");
    let settings = fs::read_to_string(guide().join(".shale/settings.json")).unwrap();
    assert!(settings.contains("\"enabled\": false"), "git sync of the guide vault is off: it lives inside the Shale repository");
    // the one home of the pictures (DEV-007): no copy next to the recipes
    assert!(guide().join("z_media/screenshots/editor.png").is_file());
    let stray: Vec<_> = fs::read_dir(root().join("docs/screenshots")).unwrap().filter_map(|e| e.ok()).filter(|e| e.path().extension().is_some_and(|x| x == "png")).collect();
    assert!(stray.is_empty(), "pictures belong in docs/userguide/z_media/screenshots, not in docs/screenshots: {stray:?}");
}
