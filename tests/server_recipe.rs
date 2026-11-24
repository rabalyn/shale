//! The server recipe of the user guide (SITE-049, SITE-050): the Forgejo Actions workflow and the
//! hook it shows are what they say they are, and the versions of Hugo and Pagefind in it are the
//! ones CI installs. The YAML is read by Python's PyYAML (there is no YAML reader in Shale); a
//! computer without python3 or PyYAML skips that part with a printed note, as the tests that run
//! Hugo do (BUILD-018).

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn note() -> String {
    std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docs/userguide/Publishing to a server.md")).unwrap()
}

/// The text of the fenced blocks of this language, in order.
fn blocks(text: &str, lang: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        match (&mut current, line.trim_end()) {
            (None, l) if l == format!("```{lang}") => current = Some(String::new()),
            (Some(_), "```") => out.push(current.take().unwrap()),
            (Some(b), l) => {
                b.push_str(l);
                b.push('\n');
            }
            _ => {}
        }
    }
    out
}

fn version_of(script: &str) -> String {
    let text = std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts").join(script)).unwrap();
    text.lines().find_map(|l| l.strip_prefix("VERSION=")).unwrap().trim().to_string()
}

// spec: SITE-049
#[test]
fn site_049_the_workflow_of_the_recipe_is_yaml_that_checks_out_with_lfs_builds_with_hugo_and_pagefind_and_swaps_the_folder_in() {
    let note = note();
    let yaml = blocks(&note, "yaml");
    assert_eq!(yaml.len(), 1, "one workflow in the note");
    let python = Command::new("python3").args(["-I", "-c", "import json, sys, yaml; print(json.dumps(yaml.safe_load(sys.stdin.read())))"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn();
    let Ok(mut child) = python else {
        println!("skipping: python3 is not installed, so the YAML of the recipe is not read");
        return;
    };
    child.stdin.take().unwrap().write_all(yaml[0].as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    if String::from_utf8_lossy(&out.stderr).contains("No module named 'yaml'") {
        println!("skipping: PyYAML is not installed, so the YAML of the recipe is not read");
        return;
    }
    assert!(out.status.success(), "the workflow is not valid YAML: {}", String::from_utf8_lossy(&out.stderr));
    let flow: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    // `on` is a boolean key to a YAML 1.1 reader
    let on = flow.get("on").or_else(|| flow.get("true")).expect("the workflow says when it runs");
    assert!(on.get("schedule").is_some() && on.get("workflow_dispatch").is_some(), "{on}");
    let steps = flow["jobs"]["site"]["steps"].as_array().expect("a job with steps");
    let checkout = &steps[0];
    assert!(checkout["uses"].as_str().unwrap().starts_with("actions/checkout@"), "{checkout}");
    assert_eq!(checkout["with"]["lfs"], true, "the files of Git LFS are checked out");
    assert_eq!(checkout["with"]["ref"], "site");
    let script = steps.iter().filter_map(|s| s["run"].as_str()).collect::<Vec<_>>().join("\n");
    for needed in ["./hugo", "./pagefind --site public", "mv -T /srv/www/notes.next /srv/www/notes", "ln -sfn"] {
        assert!(script.contains(needed), "the workflow does not run {needed:?}");
    }
    // the shell of the step is a script that parses
    let sh = Command::new("sh").arg("-n").arg("-c").arg(&script).output().unwrap();
    assert!(sh.status.success(), "{}", String::from_utf8_lossy(&sh.stderr));
}

// spec: SITE-049
#[test]
fn site_049_the_hook_of_the_recipe_parses_and_builds_with_hugo_and_pagefind_and_swaps_the_folder_in() {
    let note = note();
    let hooks = blocks(&note, "sh");
    assert_eq!(hooks.len(), 1, "one hook in the note");
    let hook = &hooks[0];
    assert!(hook.starts_with("#!/bin/sh\n"));
    let sh = Command::new("sh").arg("-n").arg("-c").arg(hook).output().unwrap();
    assert!(sh.status.success(), "{}", String::from_utf8_lossy(&sh.stderr));
    for needed in ["hugo --minify", "pagefind --site", "mv -T /srv/www/notes.next /srv/www/notes", "refs/heads/$branch"] {
        assert!(hook.contains(needed), "the hook does not run {needed:?}");
    }
}

// spec: SITE-049, SITE-048
#[test]
fn site_049_the_recipe_names_the_versions_that_ci_installs_and_the_lfs_and_the_snapshot() {
    let note = note();
    assert!(note.contains(&format!("hugo_extended_{0}_linux-$hugo_cpu", version_of("install-hugo.sh"))) && note.contains("hugo_cpu=arm64") && note.contains("hugo_cpu=amd64") && note.contains(&format!("/v{}/", version_of("install-hugo.sh"))), "the Hugo of the recipe is the pinned one");
    assert!(note.contains(&format!("pagefind-v{0}-$pagefind_cpu-unknown-linux-musl", version_of("install-pagefind.sh"))) && note.contains("pagefind_cpu=aarch64") && note.contains("pagefind_cpu=x86_64"), "the Pagefind of the recipe is the pinned one");
    // what a person is told before they rely on it
    for said in ["Git LFS", "one snapshot", "Shale writes no login", "public", "private"] {
        assert!(note.contains(said), "the note does not say {said:?}");
    }
}

// spec: SITE-050
#[test]
fn site_050_access_is_the_web_servers_and_the_note_says_that_the_site_is_public_otherwise() {
    let note = note();
    assert!(note.contains("**Shale writes no login.** The site is **public**"), "{note}");
    assert!(note.contains("basic authentication") && note.contains("OpenID Connect"));
    assert!(blocks(&note, "").iter().any(|b| b.contains("auth_basic")), "an example of the web server's own login");
}

// spec: SITE-071
#[test]
fn site_071_the_settings_and_the_guide_say_that_lfs_objects_stay_until_the_server_collects_them() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let page = std::fs::read_to_string(root.join("src/settings/publishing.rs")).unwrap();
    assert!(page.contains("stay on the server's LFS store until the server collects them"), "the Publishing settings say it");
    let guide = std::fs::read_to_string(root.join("docs/userguide/Publishing a vault.md")).unwrap();
    assert!(guide.contains("stays in the store until the server collects what nothing uses any more"), "the guide says it");
    assert!(note().contains("stay in the LFS store of your server until the\n  server collects"), "the server recipe says it");
}
