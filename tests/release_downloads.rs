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
    // the volume size is explicit (#502): hdiutil's own guess came out too small
    assert!(j.contains("du -sm dist/dmg"), "the size comes from du of the dmg folder");
    assert!(j.contains("DMG_SIZE_MB=$(( (DMG_USED_MB * 125 + 99) / 100 + 20 ))"), "content + 25 % + 20 MB, rounded up");
    assert!(j.contains("-lt 64 ]; then DMG_SIZE_MB=64"), "a floor of 64 MB");
    assert!(j.contains("echo \"dmg volume:"), "the size is logged");
    assert!(j.contains("hdiutil create -volname \"Shale $VERSION\" -srcfolder dist/dmg -size \"${DMG_SIZE_MB}m\" -ov -format UDZO \"release/$ASSET\""));
    assert!(j.find("DMG_SIZE_MB=$((").unwrap() < j.find("hdiutil create").unwrap(), "the size is computed before the image is made");
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
    // the iconset has all ten entries iconutil expects, 1024 as 512x512@2x, each from its own PNG
    let set = dir.join("shale.iconset");
    let iconset = Command::new("sh").arg(root.join("scripts/macos-app.sh")).arg("--iconset").arg(&set).output().unwrap();
    assert!(iconset.status.success(), "{}", String::from_utf8_lossy(&iconset.stderr));
    for (pixels, name) in [
        (16, "icon_16x16"),
        (32, "icon_16x16@2x"),
        (32, "icon_32x32"),
        (64, "icon_32x32@2x"),
        (128, "icon_128x128"),
        (256, "icon_128x128@2x"),
        (256, "icon_256x256"),
        (512, "icon_256x256@2x"),
        (512, "icon_512x512"),
        (1024, "icon_512x512@2x"),
    ] {
        let want = fs::read(root.join(format!("assets/icon/shale-{pixels}.png"))).unwrap();
        assert_eq!(fs::read(set.join(format!("{name}.png"))).unwrap(), want, "{name}.png is not shale-{pixels}.png");
    }
    assert_eq!(fs::read_dir(&set).unwrap().count(), 10);
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

// spec: BUILD-012, BUILD-004
#[test]
fn build_012_github_does_not_repeat_checks_that_ran_before() {
    let wf = workflow();
    let code: String = wf.lines().filter(|l| !l.trim_start().starts_with('#')).collect::<Vec<_>>().join("\n");
    assert!(!code.contains("cargo clippy") && !code.contains("cargo fmt"), "land.sh and cross-check.sh run these before main");
    let linux = job(&wf, "linux");
    assert!(!linux.contains("cargo test"), "Forgejo's build job tested this commit before the tag was mirrored");
    assert!(linux.contains("cargo build --release --locked") && linux.contains("name: Package"), "the Linux build and download stay");
    assert!(linux.contains("check-release.sh"), "the release check stays");
    for name in ["macos", "windows"] {
        assert!(job(&wf, name).contains("cargo test --release --locked"), "{name} is tested nowhere else");
    }
    let ci = fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".forgejo/workflows/ci.yml")).unwrap();
    assert!(ci.contains("cargo test --release --locked"), "the Forgejo build job tests the tagged commit");
}

fn read(path: &str) -> String {
    fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path)).unwrap()
}

// spec: BUILD-039
#[test]
fn build_039_main_check_tests_macos_and_windows_on_pushes_of_main() {
    let mc = read(".github/workflows/main-check.yml");
    let code: String = mc.lines().filter(|l| !l.trim_start().starts_with('#')).collect::<Vec<_>>().join("\n");
    assert!(code.contains("on:\n  push:\n    branches: [main]\n  workflow_dispatch:"), "main and by hand only");
    assert!(!code.contains("tags:") && !code.contains("pull_request"), "no tags, no pull requests");
    assert!(code.contains("\npermissions: {}\n"), "no permissions by default");
    assert!(code.contains("concurrency:") && code.contains("cancel-in-progress: true"), "a newer push cancels an older run");
    assert!(!code.contains("contents: write") && !code.contains("id-token") && !code.contains("upload-artifact"), "publishes nothing");
    assert!(!code.contains("cargo clippy") && !code.contains("cargo fmt"), "land.sh runs these");
    let build = workflow();
    for name in ["macos", "windows"] {
        let (m, b) = (job(&mc, name), job(&build, name));
        for cmd in ["cargo build --release --locked", "cargo test --release --locked", "RUSTFLAGS: -D warnings", "toolchain: 1.100.0"] {
            assert!(m.contains(cmd) && b.contains(cmd), "{name}: `{cmd}` differs between main-check.yml and build.yml");
        }
        assert!(m.contains("permissions:\n      contents: read"), "{name}: read-only");
    }
    assert!(job(&mc, "macos").contains("brew install ffmpeg") && job(&mc, "windows").contains("choco install ffmpeg"), "the video tests run");
    let linux = job(&mc, "linux");
    assert!(linux.contains("cargo build --release --locked") && linux.contains("cargo test --release --locked --no-run"), "Linux builds and warms its cache");
    assert!(linux.contains("runs-on: ubuntu-22.04"), "the runner of build.yml's linux job");
    assert!(!linux.contains("cargo test --release --locked\n"), "Linux tests ran on Forgejo");
}

// spec: BUILD-039
#[test]
fn build_039_both_workflows_share_the_rust_cache_key() {
    let (mc, build) = (read(".github/workflows/main-check.yml"), workflow());
    let pinned = "Swatinem/rust-cache@d1527c8f67ef9af357290475d4af4b0fab8f12e0 # v2";
    for (file, text, jobs) in [("main-check.yml", &mc, ["linux", "macos", "windows"]), ("build.yml", &build, ["linux", "macos", "windows"])] {
        for name in jobs {
            let j = job(text, name);
            assert!(j.contains(pinned), "{file} {name}: the pinned cache action");
            assert!(j.contains("shared-key: release-${{ runner.os }}"), "{file} {name}: the same shared-key, or the release build cannot restore what main saved");
            assert!(j.contains("save-if: ${{ github.ref == 'refs/heads/main' }}"), "{file} {name}: only main saves");
        }
    }
    assert_eq!(mc.matches("rust-cache@").count(), 3);
    assert_eq!(build.matches("rust-cache@").count(), 3);
}

// spec: BUILD-039
#[test]
fn build_039_a_release_tag_reaches_github_before_main_and_is_not_tested_twice() {
    let mc = read(".github/workflows/main-check.yml");
    let gate = job(&mc, "gate");
    assert!(gate.contains("git ls-remote --tags origin") && gate.contains("[0-9]+\\.[0-9]+\\.[0-9]+$"), "a release tag (no pre-release part) at the commit");
    assert!(gate.contains("[ \"$GITHUB_EVENT_NAME\" = push ]"), "a run by hand is never skipped");
    for name in ["linux", "macos", "windows"] {
        let j = job(&mc, name);
        assert!(j.contains("needs: gate") && j.contains("needs.gate.outputs.skip != 'true'"), "{name} waits for the gate");
    }
    let mirror = job(&read(".forgejo/workflows/ci.yml"), "mirror");
    let tag = mirror.find("          if [ \"$release\" = yes ]; then\n            push_tag").expect("the tag is pushed for a release");
    let main = mirror.find("git_gh push github \"$SHA:refs/heads/main\"").expect("main is pushed");
    assert!(tag < main, "the release tag must reach GitHub before main, or the gate finds no tag");
}

/// The top-level jobs of Forgejo's `ci.yml` as `(name, job-level if:, needs)`; `if:` is empty when absent.
fn forgejo_jobs() -> Vec<(String, String, Vec<String>)> {
    let text = fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".forgejo/workflows/ci.yml")).unwrap();
    let after = text.split("\njobs:\n").nth(1).expect("no `jobs:` in ci.yml");
    let mut jobs: Vec<(String, String, Vec<String>)> = Vec::new();
    for line in after.lines() {
        if let Some(name) = line.strip_prefix("  ").filter(|r| !r.starts_with(' ') && r.ends_with(':') && !r.starts_with('#')) {
            jobs.push((name.trim_end_matches(':').to_string(), String::new(), Vec::new()));
        } else if let Some(cond) = line.strip_prefix("    if:") {
            jobs.last_mut().expect("`if:` before any job").1 = cond.trim().to_string();
        } else if let Some(needs) = line.strip_prefix("    needs:") {
            let list = needs.trim().trim_start_matches('[').trim_end_matches(']');
            jobs.last_mut().expect("`needs:` before any job").2 = list.split(',').map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).collect();
        }
    }
    jobs
}

/// Whether a job-level `if:` keeps the job from running on a tag push (a push event).
fn excludes_tag_push(cond: &str) -> bool {
    cond.contains("!= 'push'") || (cond.contains("github.event_name == ") && !cond.contains("github.event_name == 'push'"))
}

// spec: BUILD-016
#[test]
fn build_016_mirror_needs_every_check_job_of_a_tag_push() {
    let jobs = forgejo_jobs();
    let names: Vec<&str> = jobs.iter().map(|j| j.0.as_str()).collect();
    let (_, mirror_if, needs) = jobs.iter().find(|j| j.0 == "mirror").expect("no job `mirror`");
    assert!(mirror_if.contains("startsWith(github.ref, 'refs/tags/v')"), "mirror no longer only runs for a v* tag: {mirror_if}");
    for n in needs {
        assert!(names.contains(&n.as_str()), "mirror needs `{n}`, which is no job of ci.yml");
    }
    for (name, cond, _) in &jobs {
        if name == "mirror" {
            continue;
        }
        assert!(!excludes_tag_push(cond), "job `{name}` does not run on a tag push (`if: {cond}`), so a mirror that needs it would be skipped");
        assert!(needs.contains(name), "mirror does not need `{name}`: a red `{name}` would not stop it (needs: {needs:?})");
    }
    for expected in ["build", "unused-deps", "coverage", "env-tests"] {
        assert!(needs.iter().any(|n| n == expected), "mirror must need `{expected}`");
    }
}
