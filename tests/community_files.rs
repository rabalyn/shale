//! The GitHub community files: issue forms, CONTRIBUTING.md and CODE_OF_CONDUCT.md (FORGE-017,
//! FORGE-018, FORGE-019).

use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(rel: &str) -> String {
    fs::read_to_string(root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

/// The relative targets of the markdown links `](target)` in `text`, without `#anchor`.
fn relative_links(text: &str) -> Vec<String> {
    text.split("](").skip(1).filter_map(|rest| rest.split(')').next()).filter(|t| !t.starts_with("http://") && !t.starts_with("https://") && !t.starts_with('#')).map(|t| t.split('#').next().unwrap_or(t).to_string()).collect()
}

fn assert_links_resolve(file: &str) {
    let links = relative_links(&read(file));
    assert!(!links.is_empty(), "{file} has no relative links");
    let missing: Vec<&String> = links.iter().filter(|l| !Path::new(&root()).join(l).exists()).collect();
    assert!(missing.is_empty(), "{file} links to missing files: {missing:?}");
}

#[test]
fn forge_017_the_issue_forms_label_bugs_and_features_and_send_security_reports_away() {
    let bug = read(".github/ISSUE_TEMPLATE/bug.yml");
    assert!(bug.contains("name: Bug report"));
    assert!(bug.lines().any(|l| l.trim() == "labels: [bug]"), "bug.yml must label `bug`");
    let feature = read(".github/ISSUE_TEMPLATE/feature.yml");
    assert!(feature.contains("name: Feature request"));
    assert!(feature.lines().any(|l| l.trim() == "labels: [kind/feature]"), "feature.yml must label `kind/feature`");
    let config = read(".github/ISSUE_TEMPLATE/config.yml");
    assert!(config.contains("blank_issues_enabled: true"));
    assert!(config.contains("https://github.com/rabalyn/shale/security/advisories/new"));
}

#[test]
fn forge_018_contributing_links_resolve() {
    assert_links_resolve("CONTRIBUTING.md");
}

#[test]
fn forge_018_contributing_names_the_rules_and_the_landing() {
    let text = read("CONTRIBUTING.md");
    for needle in ["Refs #N", "scripts/land.sh", "SHOULD BE", "-D warnings", "CODE_OF_CONDUCT.md", "SECURITY.md", "GPL-3.0-or-later", "docs/changelog-pending"] {
        assert!(text.contains(needle), "CONTRIBUTING.md does not mention {needle}");
    }
}

#[test]
fn forge_019_code_of_conduct_links_resolve() {
    // The code of conduct may have no relative link; an empty list is fine here.
    let links = relative_links(&read("CODE_OF_CONDUCT.md"));
    for l in links {
        assert!(root().join(&l).exists(), "CODE_OF_CONDUCT.md links to missing {l}");
    }
}

#[test]
fn forge_019_the_code_of_conduct_says_how_to_report() {
    let text = read("CODE_OF_CONDUCT.md");
    assert!(text.contains("https://github.com/rabalyn/shale/security/advisories/new"));
    assert!(text.contains("Report content"));
    assert!(text.contains("confidential"));
}
