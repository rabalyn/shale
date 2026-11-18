//! The release workflow's downloads for macOS and Windows (BUILD-024): the jobs pack and upload
//! them, the final `release` job attests everything and is the only one that may write.
//! (The workflow itself runs only on GitHub, so these tests read its text.)

use std::fs;
use std::path::PathBuf;

fn workflow() -> String {
    fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".github/workflows/build.yml")).unwrap()
}

/// The text of one top-level job (up to the next job, which starts with two spaces and a name).
fn job(text: &str, name: &str) -> String {
    let start = text.find(&format!("\n  {name}:\n")).unwrap_or_else(|| panic!("no job {name}")) + 1;
    let rest = &text[start + 1..];
    let end = rest
        .lines()
        .scan(0, |off, l| {
            let at = *off;
            *off += l.len() + 1;
            Some((at, l))
        })
        .skip(1)
        .find(|(_, l)| l.len() > 2 && l.starts_with("  ") && !l.starts_with("   ") && l.trim_end().ends_with(':'))
        .map(|(at, _)| at)
        .unwrap_or(rest.len());
    text[start..start + 1 + end].to_string()
}

// spec: BUILD-024
#[test]
fn build_024_macos_and_windows_pack_their_download_after_the_tests_and_check_it() {
    let wf = workflow();
    for (name, file, program, check, artifact) in [
        ("macos", "shale-${LABEL}-macos-arm64.tar.gz", "cp target/release/shale ", "shasum -a 256 -c \"$ASSET.sha256\"", "shale-macos-arm64"),
        ("windows", "shale-${LABEL}-windows-x86_64.zip", "cp target/release/shale.exe ", "sha256sum -c \"$ASSET.sha256\"", "shale-windows-x86_64"),
    ] {
        let j = job(&wf, name);
        assert!(j.contains(&format!("ASSET=\"{file}\"")), "{name}: the file is not named {file}");
        assert!(j.contains(check), "{name}: the checksum is not checked");
        assert!(j.contains(program) && j.contains("scripts/userguide-dist.sh dist/shale/docs/userguide"), "{name}: program or guide missing");
        assert!(j.contains("assets/icon/shale.svg") && !j.contains(".desktop"), "{name}: the icon, and no desktop file");
        assert!(!j.contains("codesign") && !j.contains("notar"), "{name}: unsigned");
        assert!(j.contains("shell: bash"), "{name}: the packing runs in bash");
        assert!(j.contains(&format!("name: {artifact}\n          path: release/")), "{name}: the artifact");
        let test = j.find("cargo test --release --locked").unwrap();
        assert!(test < j.find("name: Package").unwrap(), "{name}: packed before the tests");
        assert!(j.contains("permissions:\n      contents: read"), "{name}: read-only");
    }
    assert!(job(&wf, "windows").contains("7z a -tzip") && job(&wf, "windows").contains("7z t"), "the zip is made and tested with 7z");
}

// spec: BUILD-024
#[test]
fn build_024_the_release_job_needs_all_three_attests_everything_and_alone_may_write() {
    let wf = workflow();
    let rel = job(&wf, "release");
    assert!(rel.contains("needs: [linux, macos, windows]"));
    assert!(rel.contains("actions/download-artifact@") && rel.contains("merge-multiple: true"));
    for subject in ["linux", "macos", "windows"] {
        assert!(rel.contains(&format!("release/${{{{ steps.files.outputs.{subject} }}}}\n")), "{subject}: file not attested");
        assert!(rel.contains(&format!("release/${{{{ steps.files.outputs.{subject} }}}}.sha256\n")), "{subject}: checksum not attested");
        assert!(rel.contains("sha256sum -c"));
    }
    assert!(rel.contains("\"$MACOS\" \"$MACOS.sha256\" \"$WINDOWS\" \"$WINDOWS.sha256\""), "the new files are not verified against the attestation");
    assert!(rel.contains("--deny-self-hosted-runners") && rel.contains("gh release create"));
    assert!(rel.contains("xattr -dr com.apple.quarantine") && rel.contains("SmartScreen") && rel.contains("certutil -hashfile") && rel.contains("shasum -a 256 -c"), "the notes");
    // the checks and notes come first, the page last
    assert!(rel.find("release-notes.sh").unwrap() < rel.find("Attest the build").unwrap());
    // only `release` may write, and nothing is attested or published elsewhere
    for name in ["linux", "macos", "windows"] {
        let j = job(&wf, name);
        assert!(j.contains("contents: read") && !j.contains(": write"), "{name} may write");
        assert!(!j.contains("attest-build-provenance") && !j.contains("gh release"), "{name} attests or publishes");
    }
    assert_eq!(wf.matches("contents: write").count(), 1);
    assert_eq!(wf.matches("id-token: write").count(), 1);
    assert!(wf.contains("permissions: {}"));
}

// spec: BUILD-024
#[test]
fn build_024_every_action_is_pinned_by_commit() {
    for l in workflow().lines().filter(|l| l.trim_start().starts_with("- uses: ") || l.trim_start().starts_with("uses: ")) {
        let at = l.split('@').nth(1).unwrap_or_else(|| panic!("not pinned: {l}"));
        let sha = &at[..40];
        assert!(sha.chars().all(|c| c.is_ascii_hexdigit()) && at[40..].trim_start().starts_with("# "), "not pinned by commit: {l}");
    }
}
