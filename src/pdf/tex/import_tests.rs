//! Importing template sets (TEX-006): sources, the build in a temporary folder, docstrip, the
//! layout in the vault, a re-import that keeps the person's files, cancel and failure that leave
//! the vault as it was. docstrip is a fake LuaLaTeX here (Unix); the real one runs against the
//! cached pinned archive in the container tests (`env_tests`) and against GitHub in `tex_import_*`.

use super::import::*;
use std::path::Path;

#[cfg(unix)]
mod fake {
    use super::super::archive_tests::{tar_entry, tar_gz, z, zip};
    use super::super::install::Handle;
    use super::super::sets::{scan, Language, LogoStatus, Source};
    use super::super::sets_tests::vault;
    use super::*;
    use crate::gitsync::TreeLock;
    use crate::pdf::tex::compile_tests::fake::{script, Fake};
    use std::path::PathBuf;
    use std::time::Duration;

    /// Every file below `dir`, relative, sorted, folders with a trailing `/`.
    fn listing(dir: &Path) -> Vec<String> {
        fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) {
            let mut es: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().collect();
            es.sort_by_key(|e| e.file_name());
            for e in es {
                let rel = e.path().strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/");
                if e.path().is_dir() {
                    out.push(format!("{rel}/"));
                    walk(base, &e.path(), out);
                } else {
                    out.push(rel);
                }
            }
        }
        let mut out = vec![];
        walk(dir, dir, &mut out);
        out
    }

    fn request(vault: &Path, what: What, engine: Option<crate::pdf::tex::engine::Engine>) -> Request {
        Request { vault: vault.to_path_buf(), what, engine, tree: TreeLock::default(), tree_wait: Duration::from_millis(200) }
    }

    fn run(req: &Request) -> Result<Imported, String> {
        import(req, &Handle::default(), &|| {})
    }

    /// The sources of a "tuda-ci" that has only .dtx and .ins: no class file until docstrip has run.
    fn tuda_sources(dir: &Path) {
        for (name, text) in [
            ("tex/tuda-ci.ins", "\\input docstrip.tex"),
            ("tex/tuda-ci.dtx", "% the sources"),
            ("tex/tudacolors.def", "\\colors"),
            ("tex/beamerthemeTUDa.sty", "\\theme"),
            ("tex/tuda-a4paper.clo", "\\clo"),
            ("tex/notes.log", "a log"),
            ("examples/DEMO-TUDaPub.tex", "\\documentclass{tudapub}"),
            ("doc/manual.pdf", "%PDF"),
            ("README.md", "# tuda"),
            ("build.lua", "-- build"),
        ] {
            let p = dir.join(name);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
    }

    /// A LuaLaTeX that, given an `.ins`, records where and how it ran and makes the class files.
    fn docstrip_engine(f: &Fake) -> crate::pdf::tex::engine::Engine {
        let st = f.state();
        script(
            &f.dir.join("bin"),
            "lualatex",
            &format!(
                "S='{}'\npwd >> \"$S/pwd\"\necho \"$@\" >> \"$S/args\"\nenv >> \"$S/env\"\nif [ -f \"$S/sleep\" ]; then touch \"$S/started\"; sleep 30; fi\nif [ -f \"$S/exit\" ]; then echo 'docstrip says: bad' ; exit 1; fi\necho '\\ProvidesExplClass{{tudapub}}{{2026-06-08}}{{4.06}}{{Publications}}' > tudapub.cls\necho x > tudaletter.cls\necho x > tudafonts.sty",
                st.display()
            ),
        );
        f.engine()
    }

    fn tuda_folder(tag: &str) -> PathBuf {
        let d = vault(tag);
        tuda_sources(&d);
        d
    }

    fn unchanged(before: &[String], v: &Path) {
        assert_eq!(listing(v), before, "the vault is as it was");
    }

    // spec: TEX-006
    #[test]
    fn the_tuda_templates_are_built_with_docstrip_and_laid_out_in_the_vault() {
        let f = Fake::new("imp-tuda");
        let src = tuda_folder("imp-tuda-src");
        let v = vault("imp-tuda");
        let got = run(&request(&v, What::Local(src.clone()), Some(docstrip_engine(&f)))).unwrap();
        assert_eq!(got, Imported { id: "tuda".into(), name: "TU Darmstadt (tuda-ci)".into(), version: Some("4.06".into()), kinds: 11, replaced: false, logo_needed: true });
        let l = listing(&v.join("templates/tex/tuda"));
        let kinds: Vec<&String> = l.iter().filter(|n| n.starts_with("kinds/") && n.ends_with(".toml")).collect();
        assert_eq!(kinds.len(), 11, "{l:?}");
        for want in [
            "README.md",
            "set.toml",
            "settings.toml",
            "tex/",
            "tex/tudapub.cls",
            "tex/tudaletter.cls",
            "tex/tudafonts.sty",
            "tex/tudacolors.def",
            "tex/beamerthemeTUDa.sty",
            "tex/tuda-a4paper.clo",
            "kinds/pub.toml",
            "kinds/announcement.toml",
        ] {
            assert!(l.contains(&want.to_string()), "{want} in {l:?}");
        }
        // sources, logs, examples, docs and build scripts are not part of the set; the logo slot is made by "Choose logo"
        for not in ["tex/tuda-ci.ins", "tex/tuda-ci.dtx", "tex/notes.log", "examples/", "doc/", "build.lua", "logo/"] {
            assert!(!l.iter().any(|n| n.starts_with(not)), "{not} in {l:?}");
        }
        // docstrip ran on the .ins, in its folder, without a shell escape, and nothing but the work folder was written to
        assert_eq!(f.get("args").trim(), "-interaction=nonstopmode -halt-on-error -no-shell-escape tuda-ci.ins");
        assert!(f.get("pwd").trim().ends_with("/src/tex"), "{}", f.get("pwd"));
        assert!(f.get("env").lines().any(|l| l == "openout_any=p"));
        assert!(!src.join("tex/tudapub.cls").exists(), "the source folder is left alone");
        // what was written
        let found = scan(&v);
        let s = found[0].info.as_ref().unwrap();
        assert_eq!((s.meta.name.as_str(), s.meta.version.as_deref(), s.meta.logo_file.as_deref()), ("TU Darmstadt (tuda-ci)", Some("4.06"), Some("tuda_logo")));
        assert_eq!(s.meta.source, Source::Local { path: src.to_string_lossy().into_owned() });
        assert_eq!(s.meta.imported_at, crate::mail::store::today_string());
        assert_eq!((s.settings.accent.as_str(), s.settings.language, s.kinds.len(), s.problems.len()), ("9c", Language::En, 11, 0));
        assert_eq!(s.logo, LogoStatus::Missing);
        assert!(std::fs::read_to_string(v.join("templates/tex/tuda/README.md")).unwrap().contains("corporate design portal"));
        assert!(got.notice().contains("imported: 11 document kinds") && got.notice().contains("logo is missing") && got.notice().contains("4.06"), "{}", got.notice());
        // nothing is left in the folder of the sets
        assert_eq!(std::fs::read_dir(v.join("templates/tex")).unwrap().count(), 1);
        std::fs::remove_dir_all(&v).unwrap();
        std::fs::remove_dir_all(&src).unwrap();
    }

    // spec: TEX-006
    #[test]
    fn a_zip_and_a_tar_gz_with_a_folder_around_everything_are_imported_like_a_folder() {
        let f = Fake::new("imp-zip");
        let engine = docstrip_engine(&f);
        let files: [(&str, &[u8]); 4] = [("tuda-ci-4.06/tex/tuda-ci.ins", b"\\input docstrip.tex"), ("tuda-ci-4.06/tex/tuda-ci.dtx", b"x"), ("tuda-ci-4.06/tex/tudacolors.def", b"c"), ("tuda-ci-4.06/examples/e.tex", b"e")];
        let dl = vault("imp-zip-dl");
        let mut zentries = vec![z("tuda-ci-4.06/", b"")];
        zentries.extend(files.iter().map(|(n, d)| z(n, d)));
        std::fs::write(dl.join("tuda.zip"), zip(&zentries)).unwrap();
        let tentries: Vec<Vec<u8>> = files.iter().map(|(n, d)| tar_entry(n, b'0', d)).collect();
        std::fs::write(dl.join("tuda.tar.gz"), tar_gz(&tentries)).unwrap();
        for archive in ["tuda.zip", "tuda.tar.gz"] {
            let v = vault("imp-zip");
            let got = run(&request(&v, What::Local(dl.join(archive)), Some(engine.clone()))).unwrap_or_else(|e| panic!("{archive}: {e}"));
            assert_eq!((got.id.as_str(), got.kinds), ("tuda", 11), "{archive}");
            let l = listing(&v.join("templates/tex/tuda/tex"));
            assert_eq!(l, ["tudacolors.def", "tudafonts.sty", "tudaletter.cls", "tudapub.cls"], "{archive}");
            std::fs::remove_dir_all(&v).unwrap();
        }
        std::fs::remove_dir_all(&dl).unwrap();
    }

    // spec: TEX-006
    #[test]
    fn a_set_without_manifests_is_imported_with_no_kinds_and_the_notice_says_so() {
        let src = vault("imp-unknown-src");
        let src = src.join("My Fancy Templates.git");
        std::fs::create_dir_all(src.join("sty/more")).unwrap();
        std::fs::create_dir_all(src.join("examples")).unwrap();
        std::fs::write(src.join("fancy.cls"), "\\ProvidesClass{fancy}").unwrap();
        std::fs::write(src.join("sty/fancy-extra.sty"), "x").unwrap();
        std::fs::write(src.join("sty/more/deep.def"), "x").unwrap();
        std::fs::write(src.join("sty/logo.png"), "png").unwrap();
        std::fs::write(src.join("examples/e.cls"), "no").unwrap();
        let v = vault("imp-unknown");
        // no TeX is needed: the class file is there
        let got = run(&request(&v, What::Local(src.clone()), None)).unwrap();
        assert_eq!((got.id.as_str(), got.name.as_str(), got.kinds, got.version.clone(), got.logo_needed), ("my-fancy-templates", "My Fancy Templates", 0, None, false));
        let dir = v.join("templates/tex/my-fancy-templates");
        assert_eq!(listing(&dir), ["README.md", "set.toml", "settings.toml", "tex/", "tex/fancy.cls", "tex/sty/", "tex/sty/fancy-extra.sty", "tex/sty/more/", "tex/sty/more/deep.def"], "the examples and the logo picture are left out");
        assert!(!dir.join("kinds").exists());
        let n = got.notice();
        assert!(n.contains("no document kinds") && n.contains("added by hand") && n.contains("README"), "{n}");
        let readme = std::fs::read_to_string(dir.join("README.md")).unwrap();
        assert!(readme.contains("Add\na kind by hand") && readme.contains("kinds/<kind>.toml"), "{readme}");
        let s = scan(&v).remove(0).info.unwrap();
        assert!(s.kinds.is_empty() && s.settings.accent.is_empty() && s.logo == LogoStatus::NotNeeded);
        std::fs::remove_dir_all(&v).unwrap();
        std::fs::remove_dir_all(src.parent().unwrap()).unwrap();
    }

    // spec: TEX-006
    #[test]
    fn a_re_import_updates_the_files_keeps_settings_and_logo_and_trashes_the_old_set() {
        let f = Fake::new("imp-again");
        let engine = docstrip_engine(&f);
        let src = tuda_folder("imp-again-src");
        let v = vault("imp-again");
        assert!(!run(&request(&v, What::Local(src.clone()), Some(engine.clone()))).unwrap().replaced);
        let dir = v.join("templates/tex/tuda");
        // the person's work: settings, a logo, a changed kind, a changed class file
        let mine = "accent = \"3b\"\nlanguage = \"de\"\ndepartment = \"Mathematik\"\nlogo = \"tuda_logo.pdf\"\n# my note\n";
        std::fs::write(dir.join("settings.toml"), mine).unwrap();
        std::fs::create_dir_all(dir.join("logo")).unwrap();
        std::fs::write(dir.join("logo/tuda_logo.pdf"), "my logo").unwrap();
        std::fs::write(dir.join("kinds/pub.toml"), "# edited by hand\nname = \"Mine\"\nmapping = \"article\"\nclass = \"tudapub\"\n").unwrap();
        std::fs::write(dir.join("tex/tudapub.cls"), "edited class").unwrap();
        // the source has a new file
        std::fs::write(src.join("tex/tudacolors.def"), "\\new colors").unwrap();
        let got = run(&request(&v, What::Local(src.clone()), Some(engine))).unwrap();
        assert!(got.replaced && got.kinds == 11, "{got:?}");
        assert!(got.notice().contains("updated") && got.notice().contains("settings and logo were kept"), "{}", got.notice());
        assert_eq!(std::fs::read_to_string(dir.join("settings.toml")).unwrap(), mine, "settings.toml is kept byte for byte");
        assert_eq!(std::fs::read(dir.join("logo/tuda_logo.pdf")).unwrap(), b"my logo");
        assert_eq!(std::fs::read_to_string(dir.join("tex/tudacolors.def")).unwrap(), "\\new colors", "the files are the new ones");
        assert!(std::fs::read_to_string(dir.join("kinds/pub.toml")).unwrap().contains("Publication"), "the shipped kinds are back");
        assert_eq!(scan(&v)[0].info.as_ref().unwrap().logo, LogoStatus::Present("tuda_logo.pdf".into()));
        // the old set, with the hand edits, is in the trash and nothing is lost
        let day = std::fs::read_dir(v.join(".trash")).unwrap().next().unwrap().unwrap().path();
        let old = day.join("templates/tex/tuda");
        assert!(std::fs::read_to_string(old.join("kinds/pub.toml")).unwrap().contains("edited by hand"));
        assert_eq!(std::fs::read_to_string(old.join("tex/tudapub.cls")).unwrap(), "edited class");
        assert_eq!(std::fs::read(old.join("logo/tuda_logo.pdf")).unwrap(), b"my logo");
        assert_eq!(std::fs::read_dir(v.join("templates/tex")).unwrap().count(), 1, "no staging folder is left");
        // a second update the same day does not replace what is in the trash
        run(&request(&v, What::Local(src.clone()), Some(docstrip_engine(&f)))).unwrap();
        assert!(day.join("templates/tex/tuda (2)").is_dir() && old.is_dir());
        std::fs::remove_dir_all(&v).unwrap();
        std::fs::remove_dir_all(&src).unwrap();
    }

    // spec: TEX-006
    #[test]
    fn cancel_stops_docstrip_and_leaves_the_vault_and_an_existing_set_as_they_were() {
        let f = Fake::new("imp-cancel");
        let engine = docstrip_engine(&f);
        let src = tuda_folder("imp-cancel-src");
        let v = vault("imp-cancel");
        // a vault with a note and nothing else: it stays so, down to the folders
        std::fs::write(v.join("note.md"), "# n").unwrap();
        let before = listing(&v);
        f.put("sleep", "");
        let handle = Handle::default();
        let req = request(&v, What::Local(src.clone()), Some(engine.clone()));
        let h = handle.clone();
        let job = std::thread::spawn(move || import(&req, &h, &|| {}));
        for _ in 0..500 {
            if f.state().join("started").exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(f.state().join("started").exists(), "docstrip started");
        let t = std::time::Instant::now();
        handle.cancel();
        let r = job.join().unwrap();
        assert_eq!(r, Err("cancelled".into()));
        assert!(t.elapsed() < Duration::from_secs(10), "docstrip was killed, not waited for");
        unchanged(&before, &v);
        // the same for an update: the set that is there is not touched
        std::fs::remove_file(f.state().join("sleep")).unwrap();
        run(&request(&v, What::Local(src.clone()), Some(engine.clone()))).unwrap();
        let with_set = listing(&v);
        let dir = v.join("templates/tex/tuda");
        let settings = std::fs::read(dir.join("settings.toml")).unwrap();
        f.put("sleep", "");
        let _ = std::fs::remove_file(f.state().join("started"));
        let handle = Handle::default();
        let req = request(&v, What::Local(src.clone()), Some(engine));
        let h = handle.clone();
        let job = std::thread::spawn(move || import(&req, &h, &|| {}));
        while !f.state().join("started").exists() {
            std::thread::sleep(Duration::from_millis(20));
        }
        handle.cancel();
        assert_eq!(job.join().unwrap(), Err("cancelled".into()));
        assert_eq!(listing(&v), with_set);
        assert_eq!(std::fs::read(dir.join("settings.toml")).unwrap(), settings);
        std::fs::remove_dir_all(&v).unwrap();
        std::fs::remove_dir_all(&src).unwrap();
    }

    // spec: TEX-006
    #[test]
    fn a_failing_docstrip_a_missing_tex_and_a_hostile_archive_leave_the_vault_as_it_was() {
        let f = Fake::new("imp-fail");
        let engine = docstrip_engine(&f);
        let src = tuda_folder("imp-fail-src");
        let v = vault("imp-fail");
        let before = listing(&v);
        f.put("exit", "1");
        let e = run(&request(&v, What::Local(src.clone()), Some(engine.clone()))).unwrap_err();
        assert!(e.contains("docstrip on tuda-ci.ins failed") && e.contains("docstrip says: bad"), "{e}");
        unchanged(&before, &v);
        let e = run(&request(&v, What::Local(src.clone()), None)).unwrap_err();
        assert!(e.contains("need docstrip") && e.contains("no TeX is usable"), "{e}");
        unchanged(&before, &v);
        let dl = vault("imp-fail-dl");
        std::fs::write(dl.join("evil.zip"), zip(&[z("pkg/a.cls", b"x"), z("pkg/../../escape.cls", b"evil")])).unwrap();
        let e = run(&request(&v, What::Local(dl.join("evil.zip")), None)).unwrap_err();
        assert!(e.contains("climbs out"), "{e}");
        assert!(!dl.parent().unwrap().join("escape.cls").exists());
        unchanged(&before, &v);
        std::fs::write(dl.join("empty.zip"), zip(&[z("pkg/readme.md", b"nothing here")])).unwrap();
        assert!(run(&request(&v, What::Local(dl.join("empty.zip")), None)).unwrap_err().contains("no class or package files"));
        assert!(run(&request(&v, What::Local(dl.join("missing.zip")), None)).unwrap_err().contains("cannot be read"));
        unchanged(&before, &v);
        assert!(run(&request(&dl.join("no-vault"), What::Preset, None)).unwrap_err().contains("no vault"));
        std::fs::remove_dir_all(&v).unwrap();
        std::fs::remove_dir_all(&dl).unwrap();
        std::fs::remove_dir_all(&src).unwrap();
    }

    // spec: TEX-006
    #[test]
    fn a_set_folder_that_is_a_file_or_a_held_tree_lock_stops_the_import_without_a_trace() {
        let src = vault("imp-lock-src");
        std::fs::write(src.join("a.cls"), "x").unwrap();
        let v = vault("imp-lock");
        let before = listing(&v);
        let req = request(&v, What::Local(src.clone()), None);
        let _held = req.tree.try_hold().unwrap();
        let e = run(&req).unwrap_err();
        assert!(e.contains("sync is still changing files"), "{e}");
        unchanged(&before, &v);
        drop(_held);
        // "templates/tex/<id>" is a file
        let id = src.file_name().unwrap().to_string_lossy().to_lowercase();
        std::fs::create_dir_all(v.join("templates/tex")).unwrap();
        std::fs::write(v.join("templates/tex").join(&id), "i am a file").unwrap();
        let before = listing(&v);
        let e = run(&request(&v, What::Local(src.clone()), None)).unwrap_err();
        assert!(e.contains("is not a set folder"), "{e}");
        unchanged(&before, &v);
        std::fs::remove_dir_all(&v).unwrap();
        std::fs::remove_dir_all(&src).unwrap();
    }
}

// spec: TEX-006
#[test]
fn an_archive_with_the_wrong_checksum_is_refused() {
    let bytes = b"some archive";
    let right = super::archive::sha256_hex(bytes);
    assert!(verify_sha256(bytes, &right).is_ok());
    let e = verify_sha256(bytes, &"0".repeat(64)).unwrap_err();
    assert!(e.contains(&right) && e.contains("was expected") && e.contains("not imported"), "{e}");
    assert!(verify_sha256(b"other bytes", &right).is_err());
}

// spec: TEX-006
#[test]
fn the_preset_is_the_tested_commit_and_its_pin_is_the_one_of_the_test_stack() {
    assert_eq!(PRESET_COMMIT.len(), 40);
    assert!(PRESET_COMMIT.chars().all(|c| c.is_ascii_hexdigit()) && PRESET_SHA256.len() == 64 && PRESET_SHA256.chars().all(|c| c.is_ascii_hexdigit()));
    let script = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/tex-env/tex-env.sh")).unwrap();
    assert!(script.contains(&format!("TUDA_COMMIT={PRESET_COMMIT}")) && script.contains(&format!("TUDA_SHA256={PRESET_SHA256}")), "the same pin in tex-env.sh");
    assert_eq!(github_archive_url(PRESET_REPO, PRESET_COMMIT), format!("https://github.com/tudace/tuda_latex_templates/archive/{PRESET_COMMIT}.tar.gz"));
    assert!(CTAN_URL.starts_with("https://") && CTAN_URL.ends_with("/tuda-ci.zip"));
}

// spec: TEX-006
#[test]
fn github_addresses_and_references_are_checked() {
    let ok = |url: &str, r: &str| parse_github(url, r).unwrap();
    assert_eq!(ok("https://github.com/tudace/tuda_latex_templates", ""), ("tudace/tuda_latex_templates".into(), "HEAD".into()));
    assert_eq!(ok(" https://github.com/o/r.git/ ", "v1.2"), ("o/r".into(), "v1.2".into()));
    assert_eq!(ok("https://github.com/o/r/tree/main", ""), ("o/r".into(), "main".into()));
    assert_eq!(ok("https://github.com/o/r/tree/feature/x", ""), ("o/r".into(), "feature/x".into()));
    assert_eq!(ok("https://github.com/o/r/tree/main", "abc123"), ("o/r".into(), "abc123".into()), "the field wins");
    for bad in [
        "http://github.com/o/r",
        "https://gitlab.com/o/r",
        "https://github.com/o",
        "https://github.com/",
        "https://github.com/o/r/issues/3",
        "https://github.com/../r",
        "https://github.com/o/r s",
        "https://github.com/o/-r",
        "github.com/o/r",
        "https://github.com/o/r?x=1",
    ] {
        assert!(parse_github(bad, "").is_err(), "{bad}");
    }
    for bad in ["../x", "a/../b", "-x", "a b", "a;b", "/abs", "a//b", ".hidden", "x?y=1"] {
        assert!(parse_github("https://github.com/o/r", bad).is_err(), "{bad}");
    }
    assert_eq!(github_archive_url("o/r", "main"), "https://github.com/o/r/archive/main.tar.gz");
}
