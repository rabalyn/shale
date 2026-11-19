//! The release workflow's downloads for macOS (a dmg) and Windows (BUILD-024): the jobs pack and upload
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
fn build_024_windows_packs_its_zip_after_the_tests_and_checks_it() {
    let wf = workflow();
    let j = job(&wf, "windows");
    assert!(j.contains("ASSET=\"shale-${LABEL}-windows-x86_64.zip\""));
    assert!(j.contains("sha256sum -c \"$ASSET.sha256\""), "the checksum is not checked");
    assert!(j.contains("cp target/release/shale.exe ") && j.contains("scripts/userguide-dist.sh dist/shale/docs/userguide"), "program or guide missing");
    assert!(j.contains("assets/icon/shale.svg") && !j.contains(".desktop"), "the icon, and no desktop file");
    assert!(!j.contains("codesign") && !j.contains("notar"), "unsigned");
    assert!(j.contains("shell: bash"), "the packing runs in bash");
    assert!(j.contains("name: shale-windows-x86_64\n          path: release/"), "the artifact");
    assert!(j.find("cargo test --release --locked").unwrap() < j.find("name: Package").unwrap(), "packed before the tests");
    assert!(j.contains("permissions:\n      contents: read"), "read-only");
    assert!(j.contains("7z a -tzip") && j.contains("7z t"), "the zip is made and tested with 7z");
}

// spec: BUILD-024
#[test]
fn build_024_macos_packs_an_ad_hoc_signed_app_in_a_dmg_after_the_tests_and_checks_it() {
    let wf = workflow();
    let j = job(&wf, "macos");
    assert!(j.contains("ASSET=\"shale-${LABEL}-macos-arm64.dmg\""), "the file is a dmg");
    assert!(!j.contains("macos-arm64.tar.gz"), "no tarball any more");
    assert!(j.contains("sh scripts/macos-app.sh target/release/shale dist \"$VERSION\""), "the bundle comes from the script");
    assert!(j.contains("shale.icns"), "the icon is checked");
    // signed ad hoc, then verified, before the dmg is made; no developer id, no notarisation
    let sign = j.find("codesign --force --deep --sign - dist/Shale.app").expect("ad-hoc signature");
    assert!(j.find("codesign --verify --deep --strict").unwrap() > sign);
    assert!(j.find("hdiutil create").unwrap() > j.find("codesign --verify").unwrap());
    assert!(!j.contains("notarytool") && !j.contains("Developer ID"), "no notarisation");
    assert!(j.contains("ln -s /Applications dist/dmg/Applications"), "the Applications link");
    assert!(j.contains("hdiutil create -volname \"Shale $VERSION\" -srcfolder dist/dmg -ov -format UDZO \"release/$ASSET\""));
    assert!(j.contains("hdiutil verify \"release/$ASSET\""), "the dmg is verified");
    assert!(j.contains("hdiutil attach -readonly") && j.contains("Shale.app/Contents/MacOS/shale") && j.contains("hdiutil detach"), "the dmg is attached and looked into");
    assert!(j.contains("shasum -a 256 -c \"$ASSET.sha256\""), "the checksum is not checked");
    assert!(j.contains("shell: bash"));
    assert!(j.contains("name: shale-macos-arm64\n          path: release/"), "the artifact");
    assert!(j.find("cargo test --release --locked").unwrap() < j.find("name: Package").unwrap(), "packed before the tests");
    assert!(j.contains("permissions:\n      contents: read"), "read-only");
}

// spec: BUILD-024
#[test]
fn build_024_the_app_bundle_has_the_layout_and_the_info_plist_of_a_mac_app() {
    use std::process::Command;
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let dir = std::env::temp_dir().join(format!("shale-macos-app-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let program = dir.join("shale-dummy");
    fs::write(&program, "#!/bin/sh\n").unwrap();
    let icns = dir.join("any.icns");
    fs::write(&icns, "icns").unwrap();
    let out = dir.join("out");
    let run = |extra: &[&std::path::Path]| Command::new("sh").arg(root.join("scripts/macos-app.sh")).arg(&program).arg(&out).arg("1.2.3").args(extra).output().unwrap();
    let made = run(&[&icns]);
    assert!(made.status.success(), "{}", String::from_utf8_lossy(&made.stderr));
    let app = out.join("Shale.app/Contents");
    assert_eq!(fs::read(app.join("MacOS/shale")).unwrap(), fs::read(&program).unwrap());
    for f in ["LICENSE", "README.md", "CHANGELOG.md"] {
        assert!(app.join("Resources").join(f).is_file(), "{f} missing");
    }
    assert_eq!(fs::read(app.join("Resources/shale.icns")).unwrap(), b"icns");
    let plist = fs::read_to_string(app.join("Info.plist")).unwrap();
    for (key, value) in [
        ("CFBundleName", "<string>Shale</string>"),
        ("CFBundleDisplayName", "<string>Shale</string>"),
        ("CFBundleExecutable", "<string>shale</string>"),
        ("CFBundleIdentifier", "<string>io.github.rabalyn.shale</string>"),
        ("CFBundleShortVersionString", "<string>1.2.3</string>"),
        ("CFBundleVersion", "<string>1.2.3</string>"),
        ("CFBundlePackageType", "<string>APPL</string>"),
        ("CFBundleIconFile", "<string>shale</string>"),
        ("LSMinimumSystemVersion", "<string>11.0</string>"),
        ("LSApplicationCategoryType", "<string>public.app-category.productivity</string>"),
    ] {
        assert!(plist.contains(&format!("<key>{key}</key>\n\t{value}")), "{key} is not {value}:\n{plist}");
    }
    assert!(plist.contains("<key>NSHighResolutionCapable</key>\n\t<true/>"));
    // without an icon file and without iconutil (not on macOS): a warning, no icon, no icon key
    if Command::new("iconutil").arg("-h").output().is_err() && cfg!(not(target_os = "macos")) {
        let bare = run(&[]);
        assert!(bare.status.success());
        assert!(String::from_utf8_lossy(&bare.stderr).contains("no icon"));
        assert!(!out.join("Shale.app/Contents/Resources/shale.icns").exists());
        assert!(!fs::read_to_string(app.join("Info.plist")).unwrap().contains("CFBundleIconFile"));
    }
    // a dev label is no bundle version number
    let dev = Command::new("sh").arg(root.join("scripts/macos-app.sh")).arg(&program).arg(&out).arg("dev-1a2b3c4").arg(&icns).output().unwrap();
    assert!(dev.status.success());
    let plist = fs::read_to_string(app.join("Info.plist")).unwrap();
    assert!(plist.contains("<string>dev-1a2b3c4</string>") && plist.contains("<key>CFBundleVersion</key>\n\t<string>0.0.0</string>"));
    let _ = fs::remove_dir_all(&dir);
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
    assert!(
        rel.contains("xattr -dr com.apple.quarantine /Applications/Shale.app")
            && rel.contains("macos-arm64.dmg")
            && rel.contains("Open Anyway")
            && rel.contains("Keychain")
            && rel.contains("SmartScreen")
            && rel.contains("certutil -hashfile")
            && rel.contains("shasum -a 256 -c"),
        "the notes"
    );
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
