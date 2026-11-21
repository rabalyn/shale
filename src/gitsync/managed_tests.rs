//! The choice of git and what it selects (SYNC-023, SYNC-024, SYNC-026, CFG-054): the settings load
//! from old files, the manifest is read strictly, the selection for Auto, System and Managed, and the
//! look at what is installed, with fake programs (Unix) and a fake managed folder.

use super::managed::*;
use super::Git;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

fn temp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("shale-gitmanaged-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// The `PATH` entries a `Git` was given (none: it uses the one of the process).
fn path_of(g: &Git) -> Option<Vec<PathBuf>> {
    g.env.iter().find(|(k, _)| k == "PATH").map(|(_, v)| std::env::split_paths(v).collect())
}

/// A managed folder below `data`: a git (`cmd/git.exe`) and/or a git-lfs (`lfs/git-lfs`) and the manifest.
pub(super) fn managed_folder(data: &Path, git: bool, lfs: bool) -> PathBuf {
    let root = managed_root(data);
    std::fs::create_dir_all(root.join("cmd")).unwrap();
    std::fs::create_dir_all(root.join("lfs")).unwrap();
    let part = |program: &str, dir: &str, version: &str| Part { tag: format!("v{version}"), version: version.into(), program: program.into(), path: vec![dir.into()] };
    let installed = Installed { git: git.then(|| part("cmd/git.exe", "cmd", "2.50.0")), lfs: lfs.then(|| part("lfs/git-lfs", "lfs", "3.7.0")) };
    if git {
        std::fs::write(root.join("cmd").join("git.exe"), "").unwrap();
    }
    if lfs {
        std::fs::write(root.join("lfs").join("git-lfs"), "").unwrap();
    }
    std::fs::write(root.join(MANIFEST), serde_json::to_vec(&installed).unwrap()).unwrap();
    root
}

/// A folder with a file called `git` (the system git) to put in a fake `PATH`.
fn system_path(dir: &Path) -> OsString {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join(crate::pdf::tex::engine::exe_name("git")), "").unwrap();
    dir.as_os_str().to_os_string()
}

// spec: CFG-054
#[test]
fn the_git_settings_default_to_auto_and_off_load_from_old_files_and_round_trip() {
    let c = crate::config::Config::default();
    assert_eq!((c.git.choice, c.git.updates), (GitChoice::Auto, GitUpdates::Off));
    let (old, _) = crate::vault_state::parse::<crate::config::Config>(br#"{"vault": "/tmp/v"}"#).unwrap();
    assert_eq!(old.git, GitConfig::default(), "a file from before has no git settings");
    let (c, _) = crate::vault_state::parse::<crate::config::Config>(br#"{"git": {"choice": "Managed"}}"#).unwrap();
    assert_eq!((c.git.choice, c.git.updates), (GitChoice::Managed, GitUpdates::Off), "a missing field takes its default");
    let (c, _) = crate::vault_state::parse::<crate::config::Config>(br#"{"git": {"choice": "Nonsense"}}"#).unwrap();
    assert_eq!(c.git, GitConfig::default(), "a value of the wrong shape loads as the default");
    let c = crate::config::Config { git: GitConfig { choice: GitChoice::System, updates: GitUpdates::Install }, ..Default::default() };
    let text = crate::config::text::to_toml(&c);
    assert!(text.contains("[git]") && text.contains("choice = \"System\"") && text.contains("updates = \"Install\""), "{text}");
    let (back, _) = crate::config::Config::parse_text(&text).unwrap();
    assert_eq!(back.git, c.git);
}

// spec: SYNC-026
#[test]
fn the_managed_folder_is_what_its_manifest_says_and_has_the_files_it_names() {
    let data = temp("manifest");
    let root = managed_root(&data);
    assert_eq!((root.clone(), partial_root(&data), old_root(&data)), (data.join("shale").join("git"), data.join("shale").join("git.partial"), data.join("shale").join("git.old")));
    assert_eq!(Installed::read(&root), None, "no folder");
    std::fs::create_dir_all(&root).unwrap();
    assert_eq!(Installed::read(&root), None, "a folder without the manifest is no managed git");
    managed_folder(&data, true, true);
    let i = Installed::read(&root).unwrap();
    assert_eq!(i.versions_text(), "git 2.50.0, git-lfs 3.7.0");
    assert_eq!(i.git_program(&root), Some(root.join("cmd/git.exe")));
    // a part whose program is gone is dropped, the other stays
    std::fs::remove_file(root.join("cmd").join("git.exe")).unwrap();
    let i = Installed::read(&root).unwrap();
    assert!(i.git.is_none() && i.lfs.is_some());
    std::fs::remove_file(root.join("lfs").join("git-lfs")).unwrap();
    assert_eq!(Installed::read(&root), None, "nothing usable");
    // a manifest that names a path out of the folder is not followed
    let evil = Installed { git: Some(Part { tag: "v1".into(), version: "1".into(), program: "../outside".into(), path: vec![] }), lfs: None };
    std::fs::write(data.join("shale").join("outside"), "").unwrap();
    std::fs::write(root.join(MANIFEST), serde_json::to_vec(&evil).unwrap()).unwrap();
    assert_eq!(Installed::read(&root), None);
    std::fs::write(root.join(MANIFEST), "not json").unwrap();
    assert_eq!(Installed::read(&root), None);
    let _ = std::fs::remove_dir_all(&data);
}

// spec: SYNC-024
#[test]
fn auto_takes_the_system_git_first_then_the_managed_one_and_system_and_managed_take_theirs() {
    let data = temp("select");
    let sys = system_path(&data.join("sys-bin"));
    let root = managed_folder(&data, true, true);
    let system_program = data.join("sys-bin").join(crate::pdf::tex::engine::exe_name("git"));

    // Auto, both there: the system git; its git-lfs wins, the managed one is the fallback behind it
    let g = selected_git(GitChoice::Auto, Some(&data), Some(&sys));
    assert_eq!(g.program, system_program);
    let path = path_of(&g).unwrap();
    assert_eq!((path.first(), path.last()), (Some(&data.join("sys-bin")), Some(&root.join("lfs"))), "{path:?}");
    assert!(!path.contains(&root.join("cmd")), "the managed git does not come into PATH when the system one is used");
    // Auto, no system git: the managed one, its folders in front
    let none = OsString::from(data.join("empty"));
    let g = selected_git(GitChoice::Auto, Some(&data), Some(&none));
    assert_eq!(g.program, root.join("cmd/git.exe"));
    assert_eq!(path_of(&g).unwrap()[..2], [root.join("cmd"), root.join("lfs")]);
    // Auto, nothing: plain `git`, which is found missing
    let g = selected_git(GitChoice::Auto, None, Some(&none));
    assert_eq!((g.program.clone(), path_of(&g)), (PathBuf::from("git"), None));
    // System: the system git, nothing added, whatever is managed
    let g = selected_git(GitChoice::System, Some(&data), Some(&sys));
    assert_eq!((g.program.clone(), path_of(&g)), (system_program.clone(), None));
    assert_eq!(selected_git(GitChoice::System, Some(&data), Some(&none)).program, PathBuf::from("git"));
    // Managed: the managed git with its folders in front, also when a system git is there
    let g = selected_git(GitChoice::Managed, Some(&data), Some(&sys));
    assert_eq!(g.program, root.join("cmd/git.exe"));
    assert_eq!(path_of(&g).unwrap()[..2], [root.join("cmd"), root.join("lfs")]);
    // Managed where only git-lfs is managed (macOS, Linux): the system git with the managed git-lfs in front
    managed_folder(&data, false, true);
    let g = selected_git(GitChoice::Managed, Some(&data), Some(&sys));
    assert_eq!(g.program, system_program);
    assert_eq!(path_of(&g).unwrap().first(), Some(&root.join("lfs")));
    // Managed, nothing installed: the system git, or plain git
    std::fs::remove_dir_all(&root).unwrap();
    assert_eq!(selected_git(GitChoice::Managed, Some(&data), Some(&sys)).program, system_program);
    assert_eq!(selected_git(GitChoice::Managed, Some(&data), Some(&none)).program, PathBuf::from("git"));
    let _ = std::fs::remove_dir_all(&data);
}

// spec: SYNC-026
#[test]
fn the_managed_git_runs_under_the_rules_of_a_git_run() {
    let data = temp("rules");
    managed_folder(&data, true, true);
    let g = selected_git(GitChoice::Managed, Some(&data), None);
    let c = g.command(Path::new("/vault"), &[std::ffi::OsStr::new("status")]);
    let args: Vec<_> = c.get_args().collect();
    assert_eq!(args, ["-C", "/vault", "status"], "an argument list, no shell");
    let envs: Vec<_> = c.get_envs().collect();
    let var = |k: &str| envs.iter().find(|(n, _)| *n == k).map(|(_, v)| *v);
    assert_eq!(var("GIT_TERMINAL_PROMPT"), Some(Some(std::ffi::OsStr::new("0"))));
    for removed in ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE"] {
        assert_eq!(var(removed), Some(None), "{removed} is removed");
    }
    assert!(var("PATH").flatten().is_some(), "the managed folders are in the PATH of the child");
    assert_eq!(c.get_program(), managed_root(&data).join("cmd/git.exe").as_os_str());
    let _ = std::fs::remove_dir_all(&data);
}

#[cfg(unix)]
mod fake {
    use super::*;
    use crate::pdf::tex::compile_tests::fake::script;

    /// A fake git that answers `--version`; for `lfs version` it runs the `git-lfs` next to itself or the
    /// first one in its `PATH` below `data` (so a git-lfs the computer has does not change a test).
    fn fake_git(dir: &Path, version: &str, data: &Path) {
        let lfs = format!("[ -x \"${{0%/*}}/git-lfs\" ] && exec \"${{0%/*}}/git-lfs\" version\n    IFS=:; for d in $PATH; do case \"$d\" in {}/*) [ -x \"$d/git-lfs\" ] && exec \"$d/git-lfs\" version ;; esac; done; exit 1", data.display());
        script(dir, "git", &format!("case \"$*\" in\n  *\"lfs version\"*) {lfs} ;;\n  *--version*) echo \"git version {version}\" ;;\nesac"));
    }

    fn fake_lfs(dir: &Path, version: &str) {
        script(dir, "git-lfs", &format!("echo git-lfs/{version}"));
    }

    // spec: SYNC-023
    #[test]
    fn detect_tells_what_the_system_has_what_is_managed_and_what_is_in_use() {
        let data = temp("detect");
        let sys = data.join("sys-bin");
        fake_git(&sys, "2.47.0", &data);
        let path = OsString::from(&sys);
        // the system git without git-lfs
        let d = detect(GitChoice::Auto, Some(&data), Some(&path));
        assert_eq!((d.system_git.as_deref(), d.system_lfs.as_deref(), d.managed.is_none()), (Some("git version 2.47.0"), None, true));
        assert_eq!(d.git_in_use, Some(("git version 2.47.0".into(), Origin::System)));
        assert!(d.lfs_in_use.is_none());
        // a managed git-lfs next to it: Auto uses it as the fallback, and it is Shale's
        let root = managed_folder(&data, false, true);
        fake_lfs(&root.join("lfs"), "3.7.0");
        let d = detect(GitChoice::Auto, Some(&data), Some(&path));
        assert!(d.system_lfs.is_none());
        assert_eq!(d.managed.as_ref().unwrap().installed.versions_text(), "git-lfs 3.7.0");
        assert_eq!(d.lfs_in_use, Some(("git-lfs/3.7.0".into(), Origin::Managed)));
        assert_eq!(detect(GitChoice::System, Some(&data), Some(&path)).lfs_in_use, None, "System does not use it");
        // the system git-lfs wins under Auto
        fake_lfs(&sys, "3.6.0");
        let d = detect(GitChoice::Auto, Some(&data), Some(&path));
        assert_eq!((d.system_lfs.as_deref(), d.lfs_in_use), (Some("git-lfs/3.6.0"), Some(("git-lfs/3.6.0".into(), Origin::System))));
        let _ = std::fs::remove_dir_all(&data);
    }

    // spec: SYNC-024
    #[test]
    fn detect_with_a_managed_git_uses_it_for_managed_and_when_there_is_no_system_git() {
        let data = temp("detect-managed");
        let root = managed_folder(&data, true, true);
        fake_git(&root.join("cmd"), "2.56.1", &data);
        std::fs::rename(root.join("cmd").join("git"), root.join("cmd").join("git.exe")).unwrap();
        fake_lfs(&root.join("lfs"), "3.8.1");
        let nothing = OsString::from(data.join("empty"));
        for choice in [GitChoice::Auto, GitChoice::Managed] {
            let d = detect(choice, Some(&data), Some(&nothing));
            assert!(d.system_git.is_none(), "{choice:?}");
            assert_eq!(d.git_in_use, Some(("git version 2.56.1".into(), Origin::Managed)), "{choice:?}");
            assert_eq!(d.lfs_in_use, Some(("git-lfs/3.8.1".into(), Origin::Managed)), "git lfs finds the managed git-lfs through PATH ({choice:?})");
        }
        let d = detect(GitChoice::System, Some(&data), Some(&nothing));
        assert!(d.system_git.is_none() && d.managed.is_some(), "the system has none, Shale has its own");
        let _ = std::fs::remove_dir_all(&data);
    }
}
