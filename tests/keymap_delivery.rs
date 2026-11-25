//! How the keymap page reaches people (rabalyn/shale#712): a workflow publishes it on GitHub Pages and the
//! release carries it as a file. The page itself is made and checked by `keymap_view::export` (KEY-060);
//! the deployment and the release only run on GitHub, so what can be checked here is how the workflows
//! are written.

use std::fs;
use std::path::PathBuf;

fn read(file: &str) -> String {
    fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(file)).unwrap_or_else(|e| panic!("{file}: {e}"))
}

// spec: BUILD-046
#[test]
fn build_046_the_pages_workflow_deploys_the_checked_in_page_with_pinned_actions_and_least_permissions() {
    let wf = read(".github/workflows/pages.yml");
    // main (which the mirror job pushes) and by hand, and nothing else
    assert!(wf.contains("on:\n  push:\n    branches: [main]\n") && wf.contains("  workflow_dispatch:\n"));
    assert!(!wf.contains("pull_request"), "Pages never deploys a pull request");
    // no permission by default; the one job may write Pages and ask for the token, nothing else
    assert!(wf.contains("\npermissions: {}\n"));
    assert!(wf.contains("      pages: write") && wf.contains("      id-token: write") && wf.contains("      contents: read"));
    assert!(!wf.contains("contents: write"));
    // the page that is deployed is the one a test keeps up to date, alone, as index.html
    assert!(wf.contains("cp docs/keymap/index.html _site/index.html") && wf.contains("path: _site"));
    assert!(wf.contains("uses: actions/upload-pages-artifact@") && wf.contains("uses: actions/deploy-pages@"));
    // every action is pinned to a commit with its version in a comment, like the other workflows
    for line in wf.lines().filter(|l| l.trim_start().starts_with("- uses:") || l.trim_start().starts_with("uses:")) {
        let at = line.split('@').nth(1).unwrap_or_else(|| panic!("{line}: no version"));
        let sha = at.split_whitespace().next().unwrap();
        assert!(sha.len() == 40 && sha.chars().all(|c| c.is_ascii_hexdigit()), "{line}: not pinned to a commit");
        assert!(line.contains("# v"), "{line}: no version comment");
    }
    // the README gives the address
    assert!(read("README.md").contains("https://rabalyn.github.io/shale/"));
}

// spec: BUILD-047
#[test]
fn build_047_every_release_carries_the_keymap_page_with_its_checksum_and_attestation() {
    let gh = read(".github/workflows/build.yml");
    assert!(gh.contains(r#"KEYMAP="shale-keymap-${LABEL}.html""#) && gh.contains(r#"cp docs/keymap/index.html "release/$KEYMAP""#), "the page is not versioned");
    assert!(gh.contains(r#"for f in "$ASSET" "$GUIDE" "$PDF" "$KEYMAP"; do sha256sum"#), "the page has no checked .sha256 file");
    assert!(gh.contains(r#"echo "keymap=shale-keymap-${LABEL}.html" >> "$GITHUB_OUTPUT""#), "its name is not passed on to the release job");
    assert!(gh.contains("outputs.pdf }} ${{ steps.files.outputs.keymap }}"), "the release job does not check the file");
    assert!(gh.contains(r#"test "$(ls | wc -l)" = 12"#), "the release holds twelve files");
    assert!(gh.contains("release/${{ steps.files.outputs.keymap }}\n") && gh.contains("release/${{ steps.files.outputs.keymap }}.sha256\n"), "the page and its checksum are not attested");
    assert!(gh.contains(r#""$KEYMAP" "$KEYMAP.sha256"; do"#), "the page and its checksum are not verified against the attestation");
    assert_eq!(gh.matches("KEYMAP: ${{ steps.files.outputs.keymap }}").count(), 2, "the verify and publish steps do not know the page");
    assert!(gh.contains("## Keymap"), "the release notes do not name the page");
    // the release uploads everything in release/
    assert!(gh.contains("gh release upload \"$GITHUB_REF_NAME\" release/*"));
}
