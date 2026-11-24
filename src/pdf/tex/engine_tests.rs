//! Finding a LuaLaTeX (TEX-001) and the setting that chooses one (CFG-044): the choice among what
//! was found, the probe of a TeX (kernel date, biber, packages) with fake programs on Unix, and the
//! probe of the real TeX of the container (ignored unless asked for).

use super::engine::*;
use std::path::{Path, PathBuf};

fn engine(kind: Kind, missing: &[&str]) -> Engine {
    let mut e = Engine::new(kind, PathBuf::from("/tex/bin"));
    e.kernel = "2025-11-01".into();
    e.missing = missing.iter().map(|s| s.to_string()).collect();
    e
}

fn found(system: Result<Engine, &str>, managed: Result<Engine, &str>) -> Detection {
    Detection { system: system.map_err(String::from), managed: managed.map_err(String::from), managed_bytes: None }
}

// spec: TEX-001
#[test]
fn the_kernel_date_is_read_from_the_probe_output() {
    assert_eq!(parse_kernel("This is LuaHBTeX\nSHALE-KERNEL 2025-11-01\n"), Some("2025-11-01".into()));
    assert_eq!(parse_kernel("SHALE-KERNEL 2022/06/01 patch level 2"), Some("2022-06-01".into()));
    assert_eq!(parse_kernel("SHALE-KERNEL xyz"), None);
    assert_eq!(parse_kernel("no date here"), None);
    assert!("2022-06-01" >= MIN_KERNEL && "2021-11-15" < MIN_KERNEL && "2025-01-01" > MIN_KERNEL, "dates compare as text");
}

// spec: TEX-001
#[test]
fn packages_without_their_file_are_listed_as_missing() {
    let all: String = CHECKED.iter().map(|(_, f)| format!("/usr/share/texmf/{f}\n")).collect();
    assert!(missing_packages(&all).is_empty());
    let some = all.replace("/usr/share/texmf/Roboto-Regular.otf\n", "");
    assert_eq!(missing_packages(&some), ["roboto"]);
    assert_eq!(missing_packages("").len(), CHECKED.len(), "nothing found: all missing");
    // a Windows path and a file with a longer name are not the file
    assert_eq!(missing_packages("C:\\texlive\\tex\\latex\\xcolor\\xcolor.sty\nfontspec.sty.bak\n").len(), CHECKED.len() - 1);
}

// spec: TEX-022
#[test]
fn every_checked_package_is_one_of_the_package_list() {
    let list = super::install::packages();
    for (pkg, file) in CHECKED {
        assert!(list.contains(pkg), "{pkg} ({file}) is not in assets/tex/packages.txt");
    }
}

// spec: TEX-001
#[test]
fn auto_takes_the_managed_tex_first_then_a_complete_system_tex() {
    let d = found(Ok(engine(Kind::System, &[])), Ok(engine(Kind::Managed, &[])));
    assert_eq!(d.selected(TexChoice::Auto).unwrap().kind, Kind::Managed);
    let d = found(Ok(engine(Kind::System, &[])), Err("not installed"));
    assert_eq!(d.selected(TexChoice::Auto).unwrap().kind, Kind::System);
    // a system TeX with packages missing is not taken by itself
    let d = found(Ok(engine(Kind::System, &["roboto", "xcharter"])), Err("not installed"));
    let why = d.selected(TexChoice::Auto).unwrap_err();
    assert!(why.contains("misses roboto, xcharter") && why.contains("use the managed one"), "{why}");
    assert_eq!(d.system_reason(), "the TeX of this computer misses roboto, xcharter; use the managed one");
    let d = found(Err("no lualatex in PATH"), Err("not installed"));
    let why = d.selected(TexChoice::Auto).unwrap_err();
    assert!(why.contains("no lualatex in PATH") && why.contains("not installed"), "{why}");
}

// spec: TEX-001
#[test]
fn an_explicit_choice_is_used_or_explained() {
    let d = found(Ok(engine(Kind::System, &["roboto"])), Ok(engine(Kind::Managed, &[])));
    assert_eq!(d.selected(TexChoice::System).unwrap().kind, Kind::System, "asked for, so used, with its missing packages known");
    assert_eq!(d.selected(TexChoice::Managed).unwrap().kind, Kind::Managed);
    let d = found(Err("its LaTeX kernel is from 2020-10-01, Shale needs 2022-06-01 or newer"), Err("not installed"));
    assert!(d.selected(TexChoice::System).unwrap_err().contains("2020-10-01"));
    assert!(d.selected(TexChoice::Managed).unwrap_err().contains("not installed"));
}

// spec: CFG-044
#[test]
fn the_tex_settings_default_to_auto_and_ctan_load_from_old_files_and_round_trip() {
    let c = crate::config::Config::default();
    assert_eq!((c.tex.engine, c.tex.repository.as_str()), (TexChoice::Auto, "https://mirror.ctan.org/systems/texlive/tlnet"));
    let (old, _) = crate::vault_state::parse::<crate::config::Config>(br#"{"vault": "/tmp/v"}"#).unwrap();
    assert_eq!(old.tex, TexConfig::default(), "a file from before has no tex settings");
    let (c, _) = crate::vault_state::parse::<crate::config::Config>(br#"{"tex": {"engine": "Managed"}}"#).unwrap();
    assert_eq!((c.tex.engine, c.tex.repository.as_str()), (TexChoice::Managed, DEFAULT_REPOSITORY), "a missing field takes its default");
    let (c, _) = crate::vault_state::parse::<crate::config::Config>(br#"{"tex": {"engine": "Nonsense"}}"#).unwrap();
    assert_eq!(c.tex, TexConfig::default(), "a value of the wrong shape loads as the default");
    let c = crate::config::Config { tex: TexConfig { engine: TexChoice::System, repository: "https://example.org/tlnet".into() }, ..Default::default() };
    let text = crate::config::text::to_toml(&c);
    assert!(text.contains("[tex]") && text.contains("engine = \"System\"") && text.contains("repository = \"https://example.org/tlnet\""), "{text}");
    let (back, _) = crate::config::Config::parse_text(&text).unwrap();
    assert_eq!(back.tex, c.tex);
}

// spec: TEX-001
#[test]
fn the_managed_tree_is_found_by_its_lualatex_whatever_the_platform_folder_is_called() {
    let data = std::env::temp_dir().join(format!("shale-tex-managed-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data);
    let root = managed_root(&data);
    assert_eq!(root, data.join("shale").join("texlive"));
    assert_eq!(partial_root(&data), data.join("shale").join("texlive.partial"));
    assert_eq!(managed_bin_dir(&root), None, "not installed");
    std::fs::create_dir_all(root.join("bin").join("other-platform")).unwrap();
    assert_eq!(managed_bin_dir(&root), None, "a platform folder without lualatex is not one");
    let good = root.join("bin").join("x86_64-linux");
    std::fs::create_dir_all(&good).unwrap();
    std::fs::write(good.join(exe_name("lualatex")), "").unwrap();
    assert_eq!(managed_bin_dir(&root), Some(good.clone()));
    assert_eq!(find_in_path("lualatex", Some(&std::env::join_paths([Path::new("/nonexistent"), good.as_path()]).unwrap())), Some(good));
    assert_eq!(find_in_path("lualatex", Some("/nonexistent".as_ref())), None);
    assert_eq!(find_in_path("lualatex", None), None);
    let _ = std::fs::remove_dir_all(&data);
}

#[cfg(unix)]
mod fake {
    use super::super::compile_tests::fake::{script, Fake};
    use super::*;

    /// A bin folder with fake programs: lualatex says this kernel, biber answers, kpsewhich finds these files.
    fn tex(dir: &Path, kernel: &str, biber: bool, files: &[&str]) {
        script(dir, "lualatex", &format!("echo 'SHALE-KERNEL {kernel}'"));
        if biber {
            script(dir, "biber", "echo 'biber version: 2.21'");
        }
        let paths: String = files.iter().map(|f| format!("echo /texmf/{f}\n")).collect();
        script(dir, "kpsewhich", &paths);
    }

    fn all_files() -> Vec<&'static str> {
        CHECKED.iter().map(|(_, f)| *f).collect()
    }

    // spec: TEX-052
    #[test]
    fn the_managed_tex_live_is_used_although_it_misses_packages_and_they_are_listed() {
        let f = Fake::new("managed-missing");
        let mut files = all_files();
        files.retain(|f| *f != "geometry.sty");
        tex(&f.dir.join("bin"), "2025-11-01", true, &files);
        let mut managed = f.engine();
        managed.kind = Kind::Managed;
        let managed = probe(managed).unwrap();
        assert_eq!(managed.missing, ["geometry".to_string()]);
        let d = Detection { system: Err("none".into()), managed: Ok(managed), managed_bytes: None };
        assert_eq!(d.selected(TexChoice::Auto).unwrap().missing, ["geometry".to_string()], "the managed TeX Live is not refused for what it misses: the dialog offers to install it");
        let dir = f.dir.join("work");
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(missing_in(&f.engine(), &dir).unwrap(), ["geometry".to_string()]);
    }

    // spec: TEX-001
    #[test]
    fn a_new_enough_tex_with_biber_is_accepted_and_its_missing_packages_are_listed() {
        let f = Fake::new("probe-ok");
        let mut files = all_files();
        files.retain(|f| *f != "XCharter-Roman.otf");
        tex(&f.dir.join("bin"), "2025-11-01", true, &files);
        let e = probe(f.engine()).unwrap();
        assert_eq!((e.kernel.as_str(), e.missing.as_slice()), ("2025-11-01", ["xcharter".to_string()].as_slice()));
        tex(&f.dir.join("bin"), "2022-06-01", true, &all_files());
        let e = probe(f.engine()).unwrap();
        assert_eq!((e.kernel.as_str(), e.missing.is_empty()), ("2022-06-01", true), "the oldest kernel is accepted");
    }

    // spec: TEX-001
    #[test]
    fn a_tex_with_an_old_kernel_or_without_biber_is_refused_with_the_reason() {
        let f = Fake::new("probe-bad");
        tex(&f.dir.join("bin"), "2021-11-15", true, &all_files());
        let why = probe(f.engine()).unwrap_err();
        assert_eq!(why, "its LaTeX kernel is from 2021-11-15, Shale needs 2022-06-01 or newer");
        tex(&f.dir.join("bin"), "2025-11-01", false, &all_files());
        std::fs::remove_file(f.dir.join("bin").join("biber")).unwrap();
        assert_eq!(probe(f.engine()).unwrap_err(), "biber was not found next to lualatex");
        script(&f.dir.join("bin"), "lualatex", "echo 'no kernel line here'");
        assert!(probe(f.engine()).unwrap_err().starts_with("lualatex did not tell its LaTeX kernel"));
        assert!(probe(Engine::new(Kind::System, f.dir.join("none"))).unwrap_err().contains("could not be started"));
    }

    // spec: TEX-001
    #[test]
    fn detect_looks_in_path_and_in_the_managed_folder() {
        let f = Fake::new("detect");
        let sys = f.dir.join("sysbin");
        tex(&sys, "2024-06-01", true, &all_files());
        let data = f.dir.join("data");
        let managed = managed_root(&data).join("bin").join("x86_64-linux");
        tex(&managed, "2025-11-01", true, &all_files());
        let path = std::env::join_paths([f.dir.join("elsewhere"), sys.clone()]).unwrap();
        let d = detect(Some(&data), Some(&path));
        assert_eq!(d.system.as_ref().unwrap().bin_dir, sys);
        assert_eq!(d.system.as_ref().unwrap().kind, Kind::System);
        assert_eq!(d.managed.as_ref().unwrap().bin_dir, managed);
        assert_eq!(d.managed.as_ref().unwrap().kind, Kind::Managed);
        assert_eq!(d.managed_bytes, Some(std::fs::read_dir(&managed).unwrap().map(|e| e.unwrap().metadata().unwrap().len()).sum()), "the disk use of the managed tree");
        // nothing there
        let d = detect(None, Some("/nonexistent".as_ref()));
        assert_eq!(d.system.unwrap_err(), "no lualatex in PATH");
        assert_eq!(d.managed.unwrap_err(), "there is no data folder to keep it in");
        let d = detect(Some(&f.dir.join("empty")), None);
        assert_eq!((d.managed.unwrap_err().as_str(), d.managed_bytes), ("not installed", None));
    }

    // spec: TEX-001
    #[test]
    fn detection_is_a_job_that_does_not_block_the_caller() {
        let f = Fake::new("detect-job");
        script(&f.dir.join("sysbin"), "lualatex", "sleep 1\necho 'SHALE-KERNEL 2025-11-01'");
        script(&f.dir.join("sysbin"), "biber", "echo b");
        script(&f.dir.join("sysbin"), "kpsewhich", "true");
        let (data, path) = (f.dir.join("data"), f.dir.join("sysbin").into_os_string());
        let started = std::time::Instant::now();
        let rx = crate::jobs::spawn(move || detect(Some(&data), Some(&path)), || {});
        assert!(started.elapsed() < std::time::Duration::from_millis(500), "starting the job returns at once");
        let d = rx.recv_timeout(std::time::Duration::from_secs(30)).unwrap().unwrap();
        assert!(d.system.is_ok());
    }
}

// spec: TEX-001
#[test]
#[ignore = "needs the container: tests/tex-env/tex-env.sh build"]
fn tex_env_the_real_tex_passes_the_probe_with_every_package() {
    crate::env_lock::hold();
    let work = super::compile::WorkDir::new().unwrap();
    let mut e = Engine::new(Kind::System, PathBuf::new());
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/tex-env/tex-env.sh");
    e.wrapper = vec!["sh".into(), script.to_string_lossy().into_owned(), "exec".into(), work.path().to_string_lossy().into_owned(), "--".into()];
    let e = probe(e).expect("the container's TeX is usable");
    assert!(e.kernel.as_str() >= MIN_KERNEL, "{}", e.kernel);
    assert!(e.missing.is_empty(), "the image has the packages of packages.txt: missing {:?}", e.missing);
}

// spec: TEX-034
#[cfg(unix)]
#[test]
fn the_pretend_tex_of_the_screenshots_is_found_as_the_managed_one_and_makes_a_pdf() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/screenshots/fixtures/fake-tex");
    let d = detect(Some(&fixtures.join("data")), None);
    let e = d.managed.unwrap_or_else(|e| panic!("the pretend TeX Live is not usable: {e}"));
    assert_eq!((e.kind, e.kernel.as_str()), (Kind::Managed, "2025-11-01"));
    assert!(e.missing.is_empty(), "every package is there: {:?}", e.missing);
    let out = super::compile::compile_test_document(&e).unwrap_or_else(|e| panic!("{}", e.headline));
    assert!(out.pdf.starts_with(b"%PDF"));
    // the screenshots' PATH has an xdg-open that opens nothing, so the test page starts no viewer
    let opener = std::process::Command::new(fixtures.join("path/xdg-open")).arg("x.pdf").status().unwrap();
    assert!(opener.success());
}

// spec: TEX-052
#[test]
fn a_file_that_tex_could_not_find_is_traced_to_the_package_of_the_list_that_brings_it() {
    assert_eq!(package_of_file("geometry.sty"), Some("geometry"));
    assert_eq!(package_of_file("scrartcl.cls"), Some("koma-script"), "a file of the check list: its package, not its name");
    assert_eq!(package_of_file("/texmf/tex/latex/ulem/ulem.sty"), Some("ulem"), "a path is reduced to the file");
    assert_eq!(package_of_file("ltablex.sty"), Some("ltablex"), "a package of the list that is called like its file");
    assert_eq!(package_of_file("nosuch.sty"), None, "a file that is no package of the list");
    assert_eq!(package_of_file("main.aux"), None);
    assert!(BOARD_NEEDS.iter().all(|p| CHECKED.iter().any(|(c, _)| c == p)), "what a board needs is looked for by detection, or the dialog could not know it is missing");
}
