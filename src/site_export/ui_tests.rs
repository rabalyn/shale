//! Settings → Publishing (SITE-051, SITE-058, SITE-064, SITE-065, SITE-067, SITE-072, SITE-074)
//! driven through the whole window, headless: the switch and the page it opens, the count and the
//! list of the chosen notes, the address, the Publish button as a job with its progress, Cancel
//! and its result, who published last, and the question when another vault published to the
//! target. The jobs and git are the real ones; the target is a bare repository in a temporary folder.

use super::job::{Lists, PUBLISH};
use super::publish::{self, publish_tests::hermetic, Request};
use super::test_support::Fixture;
use crate::app::{Options, SettingsTab, ShaleApp};
use crate::busy::Cancel;
use crate::gitsync::Git;
use crate::ui_test_support::Headless;
use eframe::egui::PointerButton;
use std::path::{Path, PathBuf};
use std::time::Duration;

struct T {
    app: ShaleApp,
    hl: Headless,
    f: Fixture,
    remote: PathBuf,
}

fn git_ok(git: &Git, dir: &Path, args: &[&str]) -> String {
    let a: Vec<&std::ffi::OsStr> = args.iter().map(std::ffi::OsStr::new).collect();
    let o = git.run(dir, &a).unwrap();
    assert!(o.ok, "git {args:?}: {}", o.stderr);
    o.text()
}

impl T {
    /// The window on Settings → Publishing of a vault with these notes. `settings`: the `publish`
    /// members of `.shale/settings.json` (the address of the bare repository is `REMOTE` in it).
    fn new(tag: &str, settings: &str, files: &[(&str, &str)]) -> T {
        let f = Fixture::new(tag);
        let remote = f.root.join("remote.git");
        std::fs::create_dir_all(&remote).unwrap();
        git_ok(&hermetic(false), &remote, &["init", "--quiet", "--bare"]);
        if !settings.is_empty() {
            let text = settings.replace("REMOTE", &remote.to_string_lossy());
            f.file(".shale/settings.json", &format!("{{\"version\": 1, \"publish\": {{{text}}}}}"));
        }
        for (path, text) in files {
            f.file(path, text);
        }
        let mut app = ShaleApp::build(Options { screenshot: None, vault: Some(f.dir.clone()), view: None, keys: None }, false);
        app.sync.git = hermetic(false);
        app.open_settings(SettingsTab::Publishing);
        let mut t = T { app, hl: Headless::new(), f, remote };
        t.hl.set_size(eframe::egui::Vec2::new(1500.0, 1700.0));
        t.hl.settle(&mut t.app);
        t.hl.frames(&mut t.app, 3);
        t
    }

    fn shows(&mut self, text: &str) -> bool {
        self.hl.frames(&mut self.app, 3);
        self.hl.text.iter().any(|(t, _)| t.contains(text))
    }

    fn click(&mut self, label: &str) {
        let r = self.hl.find_label(&mut self.app, label);
        self.hl.click(&mut self.app, r.center(), PointerButton::Primary);
    }

    /// Frames, with a rest between them, until the publish and the look at the target are over.
    fn wait(&mut self) {
        for _ in 0..600 {
            self.hl.frames(&mut self.app, 1);
            if !self.app.publishing.running() {
                self.hl.frames(&mut self.app, 3);
                if !self.app.publishing.running() {
                    return;
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("the publish did not end");
    }

    fn tip(&self) -> String {
        git_ok(&hermetic(false), &self.remote, &["for-each-ref", "--format=%(objectname)", "refs/heads/site"])
    }
}

const ON: &str = "\"enabled\": true, \"all_notes\": true, \"url\": \"REMOTE\", \"branch\": \"site\"";

// spec: SITE-051, SITE-071, CFG-020
#[test]
fn site_051_the_publishing_tab_follows_sync_and_shows_the_rest_only_when_the_switch_is_on() {
    let all = SettingsTab::ALL;
    let at = all.iter().position(|t| *t == SettingsTab::Sync).unwrap();
    assert_eq!(all[at + 1], SettingsTab::Publishing);
    let mut t = T::new("switch", "", &[("A.md", "a note\n")]);
    assert!(t.shows("Publish this vault as a website"));
    assert!(t.shows("Publishing is off for this vault"));
    for hidden in ["Which notes", "Address of the publish repository", "Check the target"] {
        assert!(!t.shows(hidden), "{hidden} is shown while the switch is off");
    }
    assert!(!t.app.vdata.settings.publish.enabled, "off by default");
    t.click("Publish this vault as a website");
    assert!(t.app.vdata.settings.publish.enabled);
    for shown in ["Which notes", "All notes", "Branch", "Address of the publish repository", "Publish and check", "Publish", "Check the target", "Media files that Git LFS stores stay on the server's LFS store"] {
        assert!(t.shows(shown), "{shown} is not shown with the switch on");
    }
    // saved with the vault's settings (SITE-052)
    t.app.save_config();
    t.app.vdata.save().unwrap();
    assert!(std::fs::read_to_string(t.f.dir.join(".shale/settings.json")).unwrap().contains("\"enabled\": true"));
    // the tab and its rows are found by the search of the settings
    assert!(crate::settings::registry::search("publish", &crate::keys::Keymap::from_overrides_for(&Default::default(), false)).iter().any(|h| h.tab == SettingsTab::Publishing));
}

// spec: SITE-058, SITE-009
#[test]
fn site_058_the_page_counts_the_notes_the_rules_choose_and_a_button_lists_the_same_notes_as_the_dry_run() {
    let mut t = T::new("count", ON, &[("A.md", "a\n"), ("B.md", "b #pub\n"), ("Folder/C.md", "c\n"), ("Hidden.md", "---\npublish: false\n---\nh\n")]);
    assert!(t.shows("3 notes are chosen by these rules."), "{:?}", t.hl.text);
    assert!(!t.shows("Folder/C.md"), "the list is closed");
    t.click("List the notes");
    for note in ["A.md", "B.md", "Folder/C.md"] {
        assert!(t.shows(note), "{note}");
    }
    assert!(!t.shows("Hidden.md"));
    // the same notes, in the same order, as the dry run of the command
    let (mut out, mut err) = (Vec::new(), Vec::new());
    crate::site_export::cli::export(&crate::site_export::cli::Args { vault: t.f.dir.clone(), out: t.f.root.join("site"), dry_run: true }, &[], &mut out, &mut err).unwrap();
    let dry: Vec<String> = String::from_utf8(out).unwrap().lines().filter_map(|l| l.strip_prefix("note ")).map(|l| l.split(" -> ").next().unwrap().to_string()).collect();
    let listed: Vec<String> = t.app.publish_chosen().iter().map(|p| crate::vault::rel_to_string(p)).collect();
    assert_eq!(listed, dry);
    // the count follows the rules: no rule, no note (and one is written singular)
    t.click("Hide the list");
    t.click("All notes (not e-mails, events, posts and contacts, unless ticked below)");
    assert!(t.shows("0 notes are chosen by these rules."));
    t.app.vdata.settings.publish.include_tags = vec!["pub".into()];
    assert!(t.shows("1 note is chosen by these rules."));
    // the lists of the page are lines of text, trimmed and without doubles
    let mut s = t.app.vdata.settings.publish.clone();
    let lists = Lists { include_folders: " Garden \n\nGarden\nWiki/Notes\n".into(), exclude_tags: "#draft".into(), ..Lists::of(1, &s) };
    assert!(lists.apply(&mut s));
    assert_eq!((s.include_folders.clone(), s.exclude_tags.clone()), (vec!["Garden".to_string(), "Wiki/Notes".to_string()], vec!["#draft".to_string()]));
    assert!(!lists.apply(&mut s), "applying it again changes nothing");
}

// spec: SITE-064, SITE-052
#[test]
fn site_064_an_address_with_a_password_is_refused_on_the_page_and_publish_does_nothing() {
    let mut t = T::new("password", "\"enabled\": true, \"all_notes\": true, \"url\": \"https://me:secret@example.org/site.git\"", &[("A.md", "a\n")]);
    assert!(t.shows("The address has a password in it"));
    assert!(!t.hl.text.iter().any(|(s, _)| s.starts_with("The address has") && s.contains("secret")), "the message does not repeat the password");
    t.click("Publish");
    t.hl.frames(&mut t.app, 3);
    assert!(!t.app.publishing.running() && t.app.publishing.last.is_none(), "the button is off");
    // a good address takes the problem away
    t.app.vdata.settings.publish.url = t.remote.to_string_lossy().into_owned();
    t.app.publishing.url_text = None;
    assert!(!t.shows("The address has a password in it"));
    assert!(t.shows("A repository of its own, never the one this vault syncs with"));
    // and no address at all says nothing yet, and the page does not look at the server by itself in a test
    t.app.vdata.settings.publish.url.clear();
    t.app.publishing.url_text = None;
    assert!(!t.shows("No address is set"));
    assert!(!t.app.publishing.running());
}

// spec: SITE-065, SITE-067, SITE-072, SITE-076
#[test]
fn site_065_the_button_publishes_in_the_background_and_the_page_says_what_was_published_and_who_published_last() {
    let mut t = T::new("publish", ON, &[("A.md", "a note ![[@Gone/x.png]] and [[B]] and [[Missing]]\n"), ("B.md", "b\n")]);
    assert!(t.shows("Not looked at yet."));
    t.click("Publish");
    assert!(t.app.busy.is_running(&PUBLISH), "a task of the registry");
    t.wait();
    assert!(t.shows("Published at "), "{:?}", t.hl.text);
    assert!(t.shows("2 notes, 0 media files and 1 link; 2 links became plain text."));
    // every warning is listed, among them the file of outside the vault that did not resolve (SITE-067)
    assert!(t.shows("1 warning:") && t.shows("A.md: @Gone/x.png"), "{:?}", t.hl.text);
    // the target holds one snapshot of this vault, and the page says from where and when (SITE-072)
    assert_eq!(git_ok(&hermetic(false), &t.remote, &["rev-list", "--count", "site"]), "1");
    let host = crate::gitsync::host_name();
    assert!(t.shows(&format!("Last published from {host} at ")), "{:?}", t.hl.text);
    assert!(t.shows("Last published from") && !t.shows("not this one"));
    // the toast and the button are back
    assert!(t.app.toasts.iter().any(|(m, _)| m.starts_with("Published 2 notes")));
    t.click("Publish");
    t.wait();
    assert_eq!(git_ok(&hermetic(false), &t.remote, &["rev-list", "--count", "site"]), "1", "still one snapshot");
}

// spec: SITE-065
#[test]
fn site_065_cancel_stops_the_publish_and_leaves_the_target_as_it_was() {
    let mut t = T::new("cancel", ON, &[("A.md", "a note\n")]);
    // git waits until the test lets it go, so the publish runs as long as the test wants
    let gate_dir = t.f.root.join("gate");
    let mut gated = hermetic(false);
    let _gate = crate::gitsync::hold_git(&mut gated, &gate_dir);
    t.app.sync.git = gated;
    t.click("Publish");
    assert!(t.shows("Publishing the site…") && t.shows("Writing the site…"), "the task and its step: {:?}", t.hl.text);
    assert!(t.shows("Cancel"));
    t.click("Cancel");
    t.wait();
    assert!(t.shows("The publish was stopped; the target was not changed."), "{:?}", t.hl.text);
    assert!(t.tip().is_empty(), "nothing was pushed");
}

// spec: SITE-074, SITE-072
#[test]
fn site_074_a_target_of_another_vault_asks_first_and_is_replaced_only_when_the_person_says_so() {
    let mut t = T::new("takeover", ON, &[("A.md", "from this vault\n")]);
    // another vault published to the target
    let other = Fixture::with("takeover-other", "\"all_notes\": true, \"url\": \"x\"", &[("Z.md", "from the other vault\n")]);
    other.file(".shale/settings.json", &format!("{{\"version\": 1, \"publish\": {{\"enabled\": true, \"all_notes\": true, \"url\": {}, \"branch\": \"site\"}}}}", serde_json::to_string(&t.remote.to_string_lossy()).unwrap()));
    other.file(".shale/history-id", &format!("{}\n", "9a".repeat(16)));
    let theirs = publish::publish(&hermetic(false), &Request { vault: &other.dir, libraries: &[], base: &other.root.join("data"), device: "their pc", take_over: false }, &Cancel::default(), &|_| {}).unwrap();
    // who is there is read from the server
    t.click("Check the target");
    t.wait();
    assert!(t.shows("Last published from their pc at ") && t.shows("by the vault “vault”, not this one"), "{:?}", t.hl.text);
    // Publish asks, and Cancel leaves it
    t.click("Publish");
    t.wait();
    assert!(t.shows("Replace what is published there?") && t.shows("published there from their pc"), "{:?}", t.hl.text);
    assert!(t.app.shortcuts_blocked(), "the keyboard belongs to the question");
    t.click("Cancel");
    assert!(!t.shows("Replace what is published there?"));
    assert_eq!(t.tip(), theirs.commit, "nothing was pushed");
    // and so does a second Publish, which is then answered with Replace it
    t.click("Publish");
    t.wait();
    t.click("Replace it");
    t.wait();
    assert!(t.shows("Published at "), "{:?}", t.hl.text);
    assert_ne!(t.tip(), theirs.commit);
    assert!(git_ok(&hermetic(false), &t.remote, &["ls-tree", "-r", "--name-only", "site"]).contains("content/a.md"));
}

// spec: SITE-064, SYNC-077
#[test]
fn site_064_a_failed_sign_in_is_told_as_git_sync_tells_it_with_a_link_to_the_guide() {
    let mut t = T::new("signin", ON, &[("A.md", "a note\n")]);
    let script = t.f.root.join("bin/fake-git");
    crate::test_exec::write_script(&script, "#!/bin/sh\nfor a in \"$@\"; do\n  if [ \"$a\" = ls-remote ]; then echo \"fatal: Authentication failed for 'https://example.org/site.git/'\" >&2; exit 128; fi\ndone\nexec git \"$@\"\n");
    t.app.sync.git.program = script;
    t.click("Publish");
    t.wait();
    assert!(t.shows("Publishing at ") && t.shows("did not work: The server did not accept the sign-in"), "{:?}", t.hl.text);
    assert!(t.shows("How to fix this"));
    assert!(t.tip().is_empty());
    // the same words for the look at the target
    t.click("Check the target");
    t.wait();
    assert!(t.shows("Could not look at the target: The server did not accept the sign-in"), "{:?}", t.hl.text);
}
