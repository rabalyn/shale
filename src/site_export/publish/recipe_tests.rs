//! The post-receive hook of the user guide's server recipe (SITE-049), run for real: the hook text
//! is the fenced block of `docs/userguide/Publishing to a server.md` itself (only the folder
//! `/srv/www` is moved into a temporary folder), installed in a bare repository that the real
//! publish pushes to, built by the real `hugo` and `pagefind`. Hugo and Pagefind are outside
//! programs (BUILD-018): a test skips with a printed note where one is missing (CI installs them),
//! and the hook is a script of a Linux server (`mv -T`).
#![cfg(target_os = "linux")]

use super::publish_tests::hermetic;
use super::*;
use crate::site_export::hugo_tests::{hugo_here, pagefind_here, unpack};
use crate::site_export::test_support::Fixture;
use std::os::unix::fs::PermissionsExt;

const NOTE: &str = include_str!("../../../docs/userguide/Publishing to a server.md");

/// The text of the one fenced block of this language of the note.
pub(super) fn fenced(lang: &str) -> String {
    let mut blocks = Vec::new();
    let mut current: Option<String> = None;
    for line in NOTE.lines() {
        match (&mut current, line.trim_end()) {
            (None, l) if l.strip_prefix("```") == Some(lang) => current = Some(String::new()),
            (Some(_), "```") => blocks.push(current.take().unwrap()),
            (Some(b), l) => {
                b.push_str(l);
                b.push('\n');
            }
            _ => {}
        }
    }
    assert_eq!(blocks.len(), 1, "one {lang} block in the note");
    blocks.remove(0)
}

/// A vault that publishes to a bare repository whose `post-receive` is the note's hook, with the
/// web server's folder below `www`; `shims` is a folder in front of the `PATH` of the hook.
struct Server {
    f: Fixture,
    git: Git,
    remote: PathBuf,
    www: PathBuf,
    shims: PathBuf,
}

impl Server {
    fn new(tag: &str) -> Option<Server> {
        if !hugo_here() || !pagefind_here() {
            return None;
        }
        let f = Fixture::new(tag);
        let remote = f.root.join("remote.git");
        let www = f.root.join("www");
        let shims = f.root.join("shims");
        for d in [&remote, &www, &shims, &f.root.join("tmp")] {
            std::fs::create_dir_all(d).unwrap();
        }
        let mut git = hermetic(false);
        let path = format!("{}:{}", shims.display(), std::env::var("PATH").unwrap_or_default());
        for (k, v) in [("PATH", path), ("HOME", f.root.display().to_string()), ("HUGO_CACHEDIR", f.root.join("hugo-cache").display().to_string()), ("TMPDIR", f.root.join("tmp").display().to_string())] {
            git.env.push((k.into(), v.into()));
        }
        let o = git.run(&remote, &[OsStr::new("init"), OsStr::new("--quiet"), OsStr::new("--bare")]).unwrap();
        assert!(o.ok, "{}", o.stderr);
        let hook = fenced("sh");
        assert!(hook.contains("/srv/www/notes") && hook.contains("branch=site"), "the hook is the note's");
        let path = remote.join("hooks/post-receive");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, hook.replace("/srv/www", &www.display().to_string())).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        f.publish(&format!("\"all_notes\": true, \"url\": {}, \"branch\": \"site\"", serde_json::to_string(&remote.to_string_lossy()).unwrap()));
        Some(Server { f, git, remote, www, shims })
    }

    fn publish(&self) {
        let base = self.f.root.join("data");
        let r = publish(&self.git, &Request { vault: &self.f.dir, libraries: &[], base: &base, device: "laptop", take_over: false }, &Cancel::default(), &|_| {});
        assert!(r.is_ok(), "the publish failed");
    }

    /// The folder the web server serves, resolved.
    fn served(&self) -> PathBuf {
        std::fs::canonicalize(self.www.join("notes")).unwrap_or_else(|e| panic!("nothing is served: {e}"))
    }

    fn releases(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(self.www.join("notes-releases")).map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
        names.sort();
        names
    }

    /// A program in front of the real one on the hook's `PATH`.
    fn shim(&self, name: &str, script: &str) {
        let path = self.shims.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn no_shim(&self, name: &str) {
        let _ = std::fs::remove_file(self.shims.join(name));
    }
}

/// The text of the files of Pagefind's index in `dir` (`fragment` or `index`) of a served site.
fn index_text(site: &Path, dir: &str) -> String {
    std::fs::read_dir(site.join("pagefind").join(dir)).unwrap().flatten().map(|e| String::from_utf8_lossy(&unpack(&e.path())).into_owned()).collect::<Vec<_>>().join("\n")
}

// spec: SITE-049
#[test]
fn site_049_the_hook_of_the_note_builds_the_published_site_with_hugo_and_pagefind_into_the_served_folder() {
    let Some(s) = Server::new("hook-build") else { return };
    s.f.file("Harbour.md", "Zebrafish swim in the harbour.\n");
    s.f.file("Closed.md", "---\npublish: false\n---\nQuokka secrets of the closed note.\n");
    s.publish();

    let site = s.served();
    assert!(s.www.join("notes").is_symlink(), "the served folder is a link to a release");
    assert_eq!(site.parent().unwrap(), std::fs::canonicalize(s.www.join("notes-releases")).unwrap(), "the link points into the releases");
    let page = std::fs::read_to_string(site.join("harbour/index.html")).expect("the published page is built");
    assert!(page.contains("Zebrafish swim in the harbour."), "{page}");
    assert!(!site.join("closed").exists(), "an unpublished note is not on the site");
    assert!(site.join("pagefind/pagefind.js").is_file() && site.join("pagefind/pagefind-ui.js").is_file(), "the search index is there");
    let (fragments, words) = (index_text(&site, "fragment"), index_text(&site, "index"));
    assert!(fragments.contains("\"url\":\"/harbour/\"") && fragments.contains("Zebrafish swim in the harbour."), "{fragments}");
    assert!(words.contains("zebrafish") && !words.contains("quokka"), "the index holds the published words only");
    assert!(s.remote.join("hooks/post-receive").is_file());
}

// spec: SITE-049
#[test]
fn site_049_the_hook_swaps_the_folder_in_in_one_step_keeps_three_builds_and_a_failed_build_leaves_the_old_site() {
    let Some(s) = Server::new("hook-swap") else { return };
    s.f.file("Harbour.md", "Zebrafish swim in the harbour.\n");
    s.publish();
    let first = s.served();
    assert!(std::fs::read_to_string(first.join("harbour/index.html")).unwrap().contains("Zebrafish"));

    // while Pagefind runs on the new build the old site is what is served, whole
    let www = s.www.display();
    let real = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).map(|d| d.join("pagefind")).find(|p| p.is_file()).expect("pagefind is on the PATH");
    s.shim("pagefind", &format!("readlink -f '{www}/notes' > '{www}/seen'\nls '{www}/notes/harbour/index.html' '{www}/notes/pagefind/pagefind.js' >> '{www}/seen'\nexec '{}' \"$@\"", real.display()));
    s.f.file("Harbour.md", "Quokka stroll in the harbour.\n");
    s.publish();
    let seen = std::fs::read_to_string(s.www.join("seen")).expect("the build ran Pagefind");
    assert_eq!(seen.lines().next(), Some(first.to_str().unwrap()), "the old release was served while the new one was built:\n{seen}");
    assert_eq!(seen.lines().count(), 3, "and it was complete:\n{seen}");
    s.no_shim("pagefind");
    let second = s.served();
    assert_ne!(second, first);
    assert!(std::fs::read_to_string(second.join("harbour/index.html")).unwrap().contains("Quokka"));
    assert!(first.is_dir(), "the old release stays for now");

    // a build that fails changes nothing that is served, and makes no release
    s.shim("pagefind", "echo 'pagefind: broken' >&2\nexit 1");
    s.f.file("Harbour.md", "Never served.\n");
    s.publish();
    s.no_shim("pagefind");
    assert_eq!(s.served(), second, "the failed build left the served site");
    assert!(std::fs::read_to_string(second.join("harbour/index.html")).unwrap().contains("Quokka"));
    assert_eq!(s.releases().len(), 2, "no release of the failed build: {:?}", s.releases());
    assert_eq!(std::fs::read_dir(s.f.root.join("tmp")).unwrap().count(), 0, "the failed build left its working folder");

    // the three newest builds are kept
    for text in ["Third.\n", "Fourth.\n"] {
        s.f.file("Harbour.md", text);
        s.publish();
    }
    let kept = s.releases();
    assert_eq!(kept.len(), 3, "{kept:?}");
    assert!(!first.exists());
    assert!(std::fs::read_to_string(s.served().join("harbour/index.html")).unwrap().contains("Fourth."));
}
