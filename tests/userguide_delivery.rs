//! How the user guide is delivered (DEV-020): the script that copies it for the release zip and
//! the archive agrees with the program on what is left out, and both release workflows use it.
//! (The zip, the archive and the attestation themselves are only made by CI, on a tag.)

use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The three things Shale writes into a vault it opens; left out of every copy of the guide.
const NOT_DELIVERED: [&str; 3] = [".shale/state.json", ".shale/tag-words.json", ".trash"];

// spec: DEV-020
#[test]
fn dev_020_the_dist_script_and_the_program_leave_out_the_same_files() {
    let script = fs::read_to_string(root().join("scripts/userguide-dist.sh")).unwrap();
    let program = fs::read_to_string(root().join("src/userguide.rs")).unwrap();
    let ignore = fs::read_to_string(root().join(".gitignore")).unwrap();
    for n in NOT_DELIVERED {
        assert!(script.contains(&format!("--exclude='{n}'")), "scripts/userguide-dist.sh does not leave out {n}");
        assert!(program.contains(&format!("\"{n}\"")), "src/userguide.rs does not leave out {n}");
        // what Shale writes is git-ignored, so the repository holds none of it either
        let ignored = format!("/docs/userguide/{n}");
        assert!(ignore.lines().any(|l| l.trim_end_matches('/') == ignored.trim_end_matches('/')), "{n} is not git-ignored");
    }
}

#[cfg(target_os = "linux")]
fn files_below(base: &Path, dir: &Path, out: &mut Vec<String>) {
    for e in fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            files_below(base, &p, out);
        } else {
            out.push(p.strip_prefix(base).unwrap().to_string_lossy().into_owned());
        }
    }
}

// spec: DEV-020
#[cfg(target_os = "linux")]
#[test]
fn dev_020_the_dist_script_copies_the_guide_without_what_shale_writes_and_refuses_no_guide() {
    let tmp = std::env::temp_dir().join(format!("shale-dist-script-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    let src = tmp.join("docs/userguide");
    for (f, text) in [("Start here.md", "# s"), ("People/a.md", "a"), ("z_media/screenshots/x.png", "png"), (".shale/settings.json", "{}"), (".shale/state.json", "{}"), (".shale/tag-words.json", "{}"), (".trash/old.md", "old")] {
        fs::create_dir_all(src.join(f).parent().unwrap()).unwrap();
        fs::write(src.join(f), text).unwrap();
    }
    fs::create_dir_all(tmp.join("scripts")).unwrap();
    fs::copy(root().join("scripts/userguide-dist.sh"), tmp.join("scripts/userguide-dist.sh")).unwrap();
    let run = |dest: &Path| std::process::Command::new("sh").arg(tmp.join("scripts/userguide-dist.sh")).arg(dest).output().unwrap();
    let dest = tmp.join("dist/zip/userguide");
    let out = run(&dest);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let mut got = vec![];
    files_below(&dest, &dest, &mut got);
    got.sort();
    assert_eq!(got, [".shale/settings.json", "People/a.md", "Start here.md", "z_media/screenshots/x.png"]);
    // no guide, no zip: the script says so instead of packing nothing
    fs::remove_file(src.join("Start here.md")).unwrap();
    let missing = run(&tmp.join("again"));
    assert!(!missing.status.success() && String::from_utf8_lossy(&missing.stderr).contains("no guide"));
    let _ = fs::remove_dir_all(&tmp);
}

// spec: DEV-020
#[test]
fn dev_020_both_release_workflows_pack_the_guide_with_the_script_and_check_the_zip() {
    for wf in [".forgejo/workflows/ci.yml", ".github/workflows/build.yml"] {
        let text = fs::read_to_string(root().join(wf)).unwrap();
        // the guide is in the archive once, as docs/userguide, made by the script (so without
        // what Shale writes into a vault): the docs are copied, then their guide is replaced
        let copy = text.find("cp -r docs dist/shale/").unwrap_or_else(|| panic!("{wf}: docs are not copied into the archive"));
        let drop = text.find("rm -rf dist/shale/docs/userguide").unwrap_or_else(|| panic!("{wf}: the plain copy of the guide stays in the archive"));
        let made = text.find("scripts/userguide-dist.sh dist/shale/docs/userguide").unwrap_or_else(|| panic!("{wf}: the archive's guide is not made by the script"));
        assert!(copy < drop && drop < made, "{wf}: the guide of the archive is made before the docs are copied");
        assert!(!text.contains("dist/shale/userguide"), "{wf}: a second copy of the guide next to the program");
        assert!(text.contains("scripts/userguide-dist.sh dist/zip/userguide"), "{wf}: no zip of the guide");
        assert!(text.contains("shale-userguide"), "{wf}: the zip is not named");
        assert!(text.contains("unzip -tq"), "{wf}: the zip is not checked");
    }
    let gh = fs::read_to_string(root().join(".github/workflows/build.yml")).unwrap();
    assert!(gh.contains("shale-userguide-${LABEL}.zip"), "the release asset is versioned");
    assert!(gh.contains("steps.files.outputs.guide }}.sha256"), "the zip's checksum is attested");
    assert!(gh.contains("sha256sum -c %s.sha256` (the same for the user guide") && gh.contains("The user guide zip"), "the release notes say how to check the zip (BUILD-011)");
    assert!(gh.contains("\"$GUIDE\" \"$GUIDE.sha256\"; do"), "the zip and its checksum are verified against the attestation");
}
