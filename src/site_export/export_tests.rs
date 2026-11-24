//! The rules of docs/spec/site-export.md, one test each: what is published, how links and media are
//! written, and what the data files hold. They look at the tree an export would write
//! (`plan::build`), which `cli_tests.rs` writes and `hugo_tests.rs` builds.

use super::plan::Content;
use super::slug;
use super::test_support::{everything, has, page, paths, snapshot, text, Fixture};
use crate::media::outside::Library;
use crate::vault_state::{parse, VaultSettings, Versioned};
use serde_json::Value;

/// The front matter of a page, read back.
fn front(page: &str) -> Value {
    let json = page.strip_prefix("---\n").and_then(|p| p.split("\n---\n").next()).expect("a page starts with front matter");
    serde_json::from_str(json).unwrap()
}

/// The text of a page below its front matter.
fn body(page: &str) -> String {
    page.strip_prefix("---\n").and_then(|p| p.split_once("\n---\n")).map(|(_, b)| b.to_string()).expect("a page has front matter")
}

fn written_notes(e: &super::plan::Export) -> Vec<&str> {
    e.notes.iter().map(|(n, _)| n.as_str()).collect()
}

// spec: SITE-001
#[test]
fn site_001_a_vault_with_no_chosen_note_writes_the_hello_world_page_and_nothing_of_the_vault() {
    let f = Fixture::with("s001", "\"include_tags\": [\"public\"]", &[("Plan.md", "Nothing is tagged.\n")]);
    let e = f.export();
    assert!(e.notes.is_empty() && e.media.is_empty());
    assert!(e.empty_because.as_deref().is_some_and(|w| w.contains("No note is chosen")), "{:?}", e.empty_because);
    assert_eq!(paths(&e).into_iter().filter(|p| p.starts_with("content/")).collect::<Vec<_>>(), ["content/_index.md"], "one page, the index");
    assert!(has(&e, "hugo.toml") && has(&e, "data/backlinks.json"), "still a complete site");
    let page = text(&e, "content/_index.md");
    assert!(page.contains("Hello from Shale") && page.contains("published with Shale") && page.contains("No notes are chosen for publishing yet.") && page.contains(" UTC."), "{page}");
    // nothing of the vault, its notes or this computer is in any file of the tree (SITE-010)
    let all = everything(&e);
    for secret in ["Plan", "Nothing is tagged", f.root.to_str().unwrap(), &crate::gitsync::host_name(), "shale-site-s001"] {
        assert!(!all.contains(secret), "{secret:?} is in the site");
    }
    assert!(text(&e, "hugo.toml").starts_with("title = \"Shale site\""));
    // with a chosen note there is no hello-world page
    f.file("Plan.md", "#public Now chosen.\n");
    let e = f.export();
    assert_eq!(written_notes(&e), ["Plan.md"]);
    assert!(!everything(&e).contains("Hello from Shale"));
}

// spec: SITE-002, SITE-051
#[test]
fn site_051_publishing_is_off_by_default_and_says_so() {
    let f = Fixture::new("s051");
    f.file("Plan.md", "Hello\n");
    // no settings file at all
    let e = f.export();
    assert!(e.notes.is_empty());
    assert!(e.empty_because.as_deref().is_some_and(|w| w.contains("Publishing is off for this vault")));
    // rules that would choose everything, but the switch is off
    f.file(".shale/settings.json", r#"{"version": 1, "publish": {"enabled": false, "all_notes": true}}"#);
    let e = f.export();
    assert!(e.notes.is_empty() && e.empty_because.as_deref().is_some_and(|w| w.contains("off")));
    // on: the rule chooses it
    f.publish("\"all_notes\": true");
    assert_eq!(written_notes(&f.export()), ["Plan.md"]);
}

// spec: SITE-052
#[test]
fn site_052_the_rules_are_in_the_vault_settings_and_a_vault_that_never_published_has_none() {
    let json = br#"{"version": 1, "publish": {"enabled": true, "all_notes": true, "include_folders": ["Blog"], "include_tags": ["pub"], "exclude_folders": ["Blog/Drafts"], "exclude_tags": ["secret"], "private_types": ["email"], "url": "https://git.example.org/me/site.git", "branch": "main", "later": 1}}"#;
    let (s, newer) = parse::<VaultSettings>(json).unwrap();
    assert!(!newer);
    let p = &s.publish;
    assert!(p.enabled && p.all_notes);
    assert_eq!((p.include_folders.as_slice(), p.include_tags.as_slice()), (&["Blog".to_string()][..], &["pub".to_string()][..]));
    assert_eq!((p.exclude_folders.as_slice(), p.exclude_tags.as_slice(), p.private_types.as_slice()), (&["Blog/Drafts".to_string()][..], &["secret".to_string()][..], &["email".to_string()][..]));
    assert_eq!((p.url.as_str(), p.branch.as_str()), ("https://git.example.org/me/site.git", "main"));
    // a key of a newer Shale stays when the file is written again (CFG-007)
    assert_eq!(serde_json::to_value(&s).unwrap()["publish"]["later"], 1);
    // the defaults: off, and no `publish` key in the file
    let (old, _) = parse::<VaultSettings>(br#"{"version": 1}"#).unwrap();
    assert!(!old.publish.enabled && !old.publish.all_notes);
    assert!(serde_json::to_value(&old).unwrap().get("publish").is_none(), "an old vault's file does not change");
    assert_eq!(VaultSettings::CURRENT, 1, "a new field with a default is no new file version");
}

// spec: SITE-002, SITE-053
#[test]
fn site_053_include_rules_combine_and_any_one_chooses_a_note() {
    let files = [("Blog/Post.md", "x\n"), ("Blog/Deep/Post2.md", "x\n"), ("Tagged.md", "Has #pub in it\n"), ("Front.md", "---\ntags: [pub]\n---\nx\n"), ("Other.md", "x\n")];
    let only = |rules: &str| {
        let f = Fixture::with("s053", rules, &files);
        let mut n: Vec<String> = f.export().notes.into_iter().map(|n| n.0).collect();
        n.sort();
        n
    };
    assert_eq!(only("\"include_folders\": [\"Blog\"]"), ["Blog/Deep/Post2.md", "Blog/Post.md"], "a folder and its subfolders");
    assert_eq!(only("\"include_tags\": [\"pub\"]"), ["Front.md", "Tagged.md"], "inline and front matter tags");
    assert_eq!(only("\"include_folders\": [\"Blog\"], \"include_tags\": [\"pub\"]"), ["Blog/Deep/Post2.md", "Blog/Post.md", "Front.md", "Tagged.md"]);
    assert_eq!(only("\"all_notes\": true").len(), 5);
}

// spec: SITE-054
#[test]
fn site_054_a_nested_tag_matches_the_rule_of_its_parent() {
    let f = Fixture::with("s054", "\"include_tags\": [\"#a\"]", &[("Child.md", "text #a/b\n"), ("Same.md", "text #a\n"), ("Near.md", "text #ab\n"), ("Up.md", "text #x/a\n")]);
    let mut n = written_notes(&f.export()).into_iter().map(String::from).collect::<Vec<_>>();
    n.sort();
    assert_eq!(n, ["Child.md", "Same.md"], "`a/b` is below `a`, `ab` and `x/a` are not");
    let f = Fixture::with("s054b", "\"include_tags\": [\"a/b\"]", &[("Parent.md", "text #a\n"), ("Child.md", "text #a/b\n")]);
    assert_eq!(written_notes(&f.export()), ["Child.md"], "a rule for the child does not take the parent");
}

// spec: SITE-055
#[test]
fn site_055_exclude_rules_win_over_include_rules() {
    let f = Fixture::with(
        "s055",
        "\"all_notes\": true, \"include_tags\": [\"pub\"], \"exclude_folders\": [\"Private\"], \"exclude_tags\": [\"hold\"]",
        &[("Private/A.md", "#pub\n"), ("Private/Sub/B.md", "x\n"), ("Held.md", "#pub #hold/x\n"), ("Fine.md", "#pub\n"), ("PrivateNot.md", "x\n")],
    );
    let mut n = written_notes(&f.export()).into_iter().map(String::from).collect::<Vec<_>>();
    n.sort();
    assert_eq!(n, ["Fine.md", "PrivateNot.md"]);
}

// spec: SITE-056
#[test]
fn site_056_all_notes_leaves_out_the_private_types_unless_they_are_ticked() {
    let files = [
        ("Mail.md", "---\ntype: email\n---\nsecret mail\n"),
        ("Event.md", "---\ntype: Event\n---\nx\n"),
        ("Post.md", "---\ntype: post\n---\nx\n"),
        ("Adressbuch/Ada.md", "no type here\n"),
        ("Person.md", "---\ntype: person\n---\nx\n"),
        ("Note.md", "---\ntype: recipe\n---\nx\n"),
    ];
    let f = Fixture::with("s056", "\"all_notes\": true", &files);
    assert_eq!(written_notes(&f.export()), ["Note.md"]);
    let f = Fixture::with("s056b", "\"all_notes\": true, \"private_types\": [\"email\", \"person\"]", &files);
    let mut n = written_notes(&f.export()).into_iter().map(String::from).collect::<Vec<_>>();
    n.sort();
    assert_eq!(n, ["Adressbuch/Ada.md", "Mail.md", "Note.md", "Person.md"], "ticked types come in, the others stay out");
    // a rule that names the notes (a folder, a tag) chooses them as it says
    let f = Fixture::with("s056c", "\"include_folders\": [\"Inbox\"]", &[("Inbox/Mail.md", "---\ntype: email\n---\nx\n")]);
    assert_eq!(written_notes(&f.export()), ["Inbox/Mail.md"]);
}

// spec: SITE-057
#[test]
fn site_057_a_notes_own_publish_property_wins_over_every_rule() {
    let f = Fixture::with(
        "s057",
        "\"all_notes\": true, \"exclude_tags\": [\"hold\"]",
        &[("Off.md", "---\npublish: false\n---\nx\n"), ("On.md", "---\npublish: true\n---\n#hold\n"), ("Rule.md", "x\n"), ("Odd.md", "---\npublish: maybe\n---\nx\n")],
    );
    let mut n = written_notes(&f.export()).into_iter().map(String::from).collect::<Vec<_>>();
    n.sort();
    assert_eq!(n, ["Odd.md", "On.md", "Rule.md"]);
    // still subject to the file rules: a conflict copy with publish: true is never a page
    f.file("On (conflict 2026-11-20 1005).md", "---\npublish: true\n---\nx\n");
    assert!(!written_notes(&f.export()).iter().any(|n| n.contains("conflict")));
    assert!(!page(&f.export(), "on.md").contains("publish"), "the property itself is not written");
}

// spec: SITE-016, SITE-059
#[test]
fn site_016_the_export_changes_nothing_of_the_vault() {
    let f = Fixture::with("s016", "\"all_notes\": true", &[("A.md", "see [[B]] and ![[p.png]]\n"), ("B.md", "b\n"), ("Folder/C.md", "c\n")]);
    f.bytes("p.png", b"\x89PNG not really");
    let before = snapshot(&f.dir);
    let e = f.export();
    assert_eq!(e.notes.len(), 3);
    assert_eq!(before, snapshot(&f.dir), "no note, no .shale file and no timestamp changed; none was added");
    assert!(!f.dir.join(".shale/state.json").exists());
}

// spec: SITE-003
#[test]
fn site_003_hidden_files_and_folders_are_never_written() {
    let f = Fixture::with("s003", "\"all_notes\": true", &[("Open.md", "[[Hidden]] ![[.pic.png]] ![[.trash/x.png]] [[.shale/settings]]\n"), (".trash/Hidden.md", "trashed words\n"), (".git/Hidden2.md", "x\n")]);
    f.bytes(".pic.png", b"png");
    f.bytes(".trash/x.png", b"png");
    let e = f.export();
    assert_eq!(written_notes(&e), ["Open.md"]);
    let all = everything(&e);
    for needle in ["trashed words", "\"publish\""] {
        assert!(!all.contains(needle), "{needle} is in the tree");
    }
    assert!(e.media.is_empty());
    assert!(!paths(&e).iter().any(|p| p.contains(".trash") || p.contains(".pic") || p.contains(".shale") || p.contains(".git")), "{:?}", paths(&e));
}

// spec: SITE-004
#[test]
fn site_004_conflict_copies_are_never_written_also_when_a_published_note_links_them() {
    let f = Fixture::with("s004", "\"all_notes\": true", &[("Plan.md", "![[pic (conflict 2026-11-20 1005).png]] [[Plan (conflict 2026-11-20 1005)]] ![[pic.png]]\n"), ("Plan (conflict 2026-11-20 1005).md", "conflicting words\n")]);
    f.bytes("pic.png", b"png");
    f.bytes("pic (conflict 2026-11-20 1005).png", b"png");
    let e = f.export();
    assert_eq!(written_notes(&e), ["Plan.md"]);
    assert!(e.media.iter().all(|m| !m.contains("conflict")) && e.media.len() == 1, "{:?}", e.media);
    assert!(!everything(&e).contains("conflicting words"));
    assert!(!paths(&e).iter().any(|p| p.contains("conflict")));
}

// spec: SITE-005
#[test]
fn site_005_temp_and_sync_debris_is_never_written() {
    let f = Fixture::with("s005", "\"all_notes\": true", &[("Plan.md", "[a](big.pdf.part) [b](x.tmp) [c](.shale-tmp-1) ![[ok.png]]\n")]);
    for n in ["big.pdf.part", "x.tmp", ".shale-tmp-1"] {
        f.bytes(n, b"junk");
    }
    f.bytes("ok.png", b"png");
    let e = f.export();
    assert_eq!(e.media, ["static/media/files/ok.png"]);
    let all = everything(&e);
    assert!(!all.contains("junk") && !all.contains("big.pdf.part"));
}

// spec: SITE-006
#[test]
fn site_006_a_media_file_is_written_only_when_a_published_note_names_it() {
    let f = Fixture::with("s006", "\"include_tags\": [\"pub\"]", &[("Shown.md", "#pub\n![[used.png]] [the file](docs/spec.pdf) ![](folder/inner.jpg)\n"), ("Hidden.md", "![[private.png]] [x](secret.pdf)\n")]);
    for n in ["used.png", "docs/spec.pdf", "folder/inner.jpg", "unused.png", "private.png", "secret.pdf"] {
        f.bytes(n, b"data");
    }
    let e = f.export();
    let mut media = e.media.clone();
    media.sort();
    assert_eq!(media, ["static/media/files/docs/spec.pdf", "static/media/files/folder/inner.jpg", "static/media/files/used.png"]);
    assert!(!paths(&e).iter().any(|p| p.contains("private") || p.contains("unused") || p.contains("secret")));
}

// spec: SITE-007
#[test]
fn site_007_a_link_to_a_note_that_is_not_published_is_written_as_its_shown_text() {
    let f = Fixture::with(
        "s007",
        "\"include_tags\": [\"pub\"]",
        &[
            ("Open.md", "#pub\nA [[Closed|shown]], [[Closed2]], [[Closed#Heading]], [md](Closed.md), [[Nowhere]] and [[Nowhere|alias]], [[Closed3|a *b*]].\n"),
            ("Closed.md", "closed\n"),
            ("Closed2.md", "closed\n"),
            ("Closed3.md", "closed\n"),
        ],
    );
    let e = f.export();
    let b = body(&page(&e, "open.md"));
    assert!(b.contains("A shown, Closed2, Closed > Heading, md, Nowhere and alias, a \\*b\\*."), "{b}");
    assert!(!b.contains("](") && !b.contains("/closed"), "no URL: {b}");
    assert_eq!(e.plain, 7);
    assert_eq!(e.links, 0);
}

// spec: SITE-008
#[test]
fn site_008_mail_originals_are_never_written() {
    let f = Fixture::with("s008", "\"all_notes\": true", &[("Mail note.md", "[[Re hello.eml]] [meeting](cal/Meeting.ics) [x](Report.pdf)\n")]);
    f.bytes("Re hello.eml", b"From: secret@example.org\n");
    f.bytes("cal/Meeting.ics", b"BEGIN:VCALENDAR secret-event");
    f.bytes("Report.pdf", b"%PDF");
    let e = f.export();
    assert_eq!(e.media, ["static/media/files/Report.pdf"]);
    let all = everything(&e);
    assert!(!all.contains("secret@example.org") && !all.contains("secret-event"));
    assert!(!paths(&e).iter().any(|p| p.ends_with(".eml") || p.ends_with(".ics")), "{:?}", paths(&e));
}

// spec: SITE-010
#[test]
fn site_010_the_output_holds_nothing_of_an_unpublished_note() {
    let f = Fixture::with(
        "s010",
        "\"include_tags\": [\"pub\"]",
        &[
            ("Open.md", "---\nrelated: \"[[Zebra Plan]]\"\nalso: [[Zebra Plan|a friend]]\n---\n#pub\nSee [[Zebra Plan|more]] and [[Zebra Plan#Kiwi heading|more]] and [[Open]].\n"),
            ("Zebra Plan.md", "---\ntags: [zebratag]\n---\n# Kiwi heading\nZebra words #zebrainline and [[Open]] and [[Zebra Other]]\n"),
            ("Zebra Other.md", "Linked from the plan [[Zebra Plan]]\n"),
        ],
    );
    let e = f.export();
    assert_eq!(written_notes(&e), ["Open.md"]);
    let all = everything(&e).to_lowercase();
    for needle in ["zebra", "kiwi", "zebratag", "zebrainline"] {
        assert!(!all.contains(needle), "{needle} is in the tree:\n{all}");
    }
    assert_eq!(text(&e, "data/backlinks.json").trim(), "{}", "no backlink from an unpublished note");
    assert!(!text(&e, "data/graph.json").contains("Zebra") && text(&e, "data/graph.json").contains("\"links\": []"));
    let fm = front(&page(&e, "open.md"));
    assert_eq!(fm["related"], "", "a link to an unpublished note is nothing in a property");
    assert_eq!(fm["also"], "a friend");
}

// spec: SITE-017
#[test]
fn site_017_wiki_links_resolve_as_in_the_editor() {
    let f = Fixture::with(
        "s017",
        "\"all_notes\": true",
        &[("Start.md", "[[Target]] [[Target|the alias]] [[Target#Second part]] [[#Own heading]] [[Folder/Deep]] [[../Start]] [[deep]]\n\n## Own heading\n"), ("Target.md", "# First\n\n## Second part\n"), ("Folder/Deep.md", "x\n")],
    );
    let e = f.export();
    let b = body(&page(&e, "start.md"));
    assert!(b.contains("[Target](/target/)"), "{b}");
    assert!(b.contains("[the alias](/target/)"));
    assert!(b.contains("[Target > Second part](/target/#second-part)"));
    assert!(b.contains("[Own heading](/start/#own-heading)"));
    assert!(b.contains("[Deep](/folder/deep/)"), "a path link, and the name `deep` finds the same note");
    assert_eq!(b.matches("[Deep](/folder/deep/)").count(), 2);
    assert!(!b.contains("[[") && !b.contains("](../"), "{b}");
}

// spec: SITE-017
#[test]
fn site_017_a_name_that_two_notes_share_counts_as_not_published() {
    let f = Fixture::with("s017b", "\"all_notes\": true", &[("Start.md", "[[Twin]] [[A/Twin]]\n"), ("A/Twin.md", "x\n"), ("B/Twin.md", "x\n")]);
    let e = f.export();
    let b = body(&page(&e, "start.md"));
    assert!(b.starts_with("Twin [Twin](/a/twin/)"), "the shared name is text, the path is a link: {b}");
}

// spec: SITE-018
#[test]
fn site_018_markdown_links_to_notes_resolve_the_same_way_and_web_links_stay() {
    let f = Fixture::with(
        "s018",
        "\"all_notes\": true",
        &[("Folder/Start.md", "[x](Note.md) [y](Folder/Note2.md#Part Two) [z](../Note.md) [w](https://example.org/a?b=1) [m](mailto:a@b.org) [t](#top)\n"), ("Note.md", "x\n"), ("Folder/Note2.md", "## Part Two\n")],
    );
    let e = f.export();
    let b = body(&page(&e, "folder/start.md"));
    assert!(b.contains("[x](/note/)"), "{b}");
    assert!(b.contains("[y](/folder/note2/#part-two)"));
    assert!(b.contains("[z](/note/)"));
    assert!(b.contains("[w](https://example.org/a?b=1) [m](mailto:a@b.org) [t](#top)"), "{b}");
}

// spec: SITE-019
#[test]
fn site_019_a_resolved_link_is_a_standard_markdown_link_with_the_alias_or_the_title_as_text() {
    let f = Fixture::with(
        "s019",
        "\"all_notes\": true",
        &[("Start.md", "[[261117_Plan]] [[261117_Plan|go]] [[Titled]] [[Möller & Söhne]]\n"), ("261117_Plan.md", "x\n"), ("Titled.md", "---\ntitle: A better title\n---\nx\n"), ("Möller & Söhne.md", "x\n")],
    );
    let e = f.export();
    let b = body(&page(&e, "start.md"));
    assert!(b.contains("[Plan](/261117-plan/) [go](/261117-plan/) [A better title](/titled/) [Möller & Söhne](/m%C3%B6ller-s%C3%B6hne/)"), "{b}");
}

// spec: SITE-020
#[test]
fn site_020_a_heading_link_points_to_the_anchor_hugo_makes() {
    use super::anchor::anchors;
    let text = "## Plan **v2** (draft)? Ünï: a_b\n\n## Same\n\n## Same\n\n## Same-1\n\n## Same\n\n## A  B\n\n## C++ & D\n\n## 1. First\n\n## [[Link]] and `code`\n\n## ½ ² ٣\n\n## [t](http://x.y) &amp; z\n\n## \n\n## ???\n";
    let ids: Vec<String> = anchors(text).into_iter().map(|a| a.id).collect();
    // what Hugo 0.170 makes of these headings (checked by hugo_tests.rs against a real build)
    assert_eq!(ids, ["plan-v2-draft-ünï-a_b", "same", "same-1", "same-1-1", "same-2", "a--b", "c--d", "1-first", "link-and-code", "--٣", "t--z", "heading"], "{ids:?}");
}

// spec: SITE-021, SITE-026
#[test]
fn site_021_the_title_is_the_title_line_without_its_date_prefix_and_a_title_property_wins() {
    let f = Fixture::with("s021", "\"all_notes\": true", &[("261117_Plan.md", "x\n"), ("12345_Plan.md", "x\n"), ("Own.md", "---\ntitle: \"Written: title\"\n---\nx\n")]);
    let e = f.export();
    assert_eq!(front(&page(&e, "261117-plan.md"))["title"], "Plan");
    assert_eq!(front(&page(&e, "12345-plan.md"))["title"], "12345_Plan", "five digits are no date prefix");
    assert_eq!(front(&page(&e, "own.md"))["title"], "Written: title");
    assert!(f.dir.join("261117_Plan.md").is_file(), "the file keeps its name");
}

// spec: SITE-022
#[test]
fn site_022_urls_are_stable_lower_case_and_two_paths_never_give_one() {
    assert_eq!(slug::slug("Garden Plan (2026)"), "garden-plan-2026");
    assert_eq!(slug::slug("日本語 Notiz"), "日本語-notiz");
    assert_eq!(slug::slug("---"), "note");
    let files = [("Garden Plan.md", "x\n"), ("garden-plan.md", "x\n"), ("Tags.md", "x\n"), ("Blog.md", "x\n"), ("Blog/Post.md", "x\n"), ("A b/C.md", "x\n"), ("a-b/C.md", "x\n")];
    let f = Fixture::with("s022", "\"all_notes\": true", &files);
    let (one, two) = (f.export(), f.export());
    let urls = |e: &super::plan::Export| {
        let mut u: Vec<String> = e.files.iter().filter(|p| p.path.starts_with("content/")).map(|p| p.path.clone()).collect();
        u.sort();
        u
    };
    assert_eq!(urls(&one), urls(&two), "the same on every export");
    let list = urls(&one);
    let mut unique = list.clone();
    unique.dedup();
    assert_eq!(list, unique, "no two paths give one page");
    assert!(list.iter().all(|p| *p == p.to_lowercase()), "lower case: {list:?}");
    assert!(list.contains(&"content/blog/post.md".to_string()) && list.contains(&"content/blog/_index.md".to_string()), "a folder keeps its slug against a note of its name: {list:?}");
    assert!(list.iter().filter(|p| p.starts_with("content/garden-plan")).count() == 2 && !list.contains(&"content/garden-plan.md".to_string()), "a clash gives both a suffix: {list:?}");
    assert!(!list.contains(&"content/tags.md".to_string()), "`tags` is the site's own: {list:?}");
    // the folder note of Blog: no, `Blog.md` is next to the folder; it is a note of its own, with a suffix
    assert_eq!(list.iter().filter(|p| p.starts_with("content/blog-")).count(), 1, "{list:?}");
}

// spec: SITE-023
#[test]
fn site_023_the_tags_of_the_front_matter_and_the_text_are_written_once_each_as_tags() {
    let f = Fixture::with("s023", "\"all_notes\": true", &[("T.md", "---\ntags: [alpha, beta]\n---\nText #beta #gamma/sub and `#code` here\n"), ("None.md", "no tags\n")]);
    let e = f.export();
    assert_eq!(front(&page(&e, "t.md"))["tags"], serde_json::json!(["alpha", "beta", "gamma/sub"]));
    assert!(front(&page(&e, "none.md")).get("tags").is_none());
}

// spec: SITE-024, SITE-025
#[test]
fn site_024_keys_hugo_reserves_move_under_shale_and_the_type_goes_to_shale_type() {
    let f = Fixture::with("s024", "\"all_notes\": true", &[("N.md", "---\ndate: 2099-01-01\ndraft: true\ntype: recipe\nlayout: wide\nurl: /elsewhere/\nslug: other\nweight: 3\naliases: [old]\nstatus: open\nrelated: [one, two]\n---\nx\n")]);
    let fm = front(&page(&f.export(), "n.md"));
    for key in ["date", "draft", "type", "layout", "url", "slug", "weight", "aliases"] {
        assert!(fm.get(key).is_none(), "{key} is at the top: {fm}");
    }
    assert_eq!(fm["shale"]["type"], "recipe");
    assert_eq!(fm["shale"]["date"], "2099-01-01");
    assert_eq!(fm["shale"]["aliases"], "old", "a list of one is its one value");
    assert_eq!(
        (fm["shale"]["draft"].as_str(), fm["shale"]["url"].as_str(), fm["shale"]["slug"].as_str(), fm["shale"]["weight"].as_str(), fm["shale"]["layout"].as_str()),
        (Some("true"), Some("/elsewhere/"), Some("other"), Some("3"), Some("wide"))
    );
    assert_eq!((fm["status"].as_str(), fm["related"].clone()), (Some("open"), serde_json::json!(["one", "two"])), "other properties stay as they are");
}

// spec: SITE-027, SITE-031, SITE-032
#[test]
fn site_027_marks_spans_and_aligned_blocks_survive_and_page_breaks_do_not() {
    let note = "==plain== <mark style=\"background: #FFFF00;\">yellow</mark> <span style=\"color: #FF0000;\">red</span> <u>u</u> H<sub>2</sub>O x<sup>2</sup>\n\n<div style=\"page-break-after: always;\"></div>\n\n<div align=\"center\">\nCentred **text**\n</div>\n\n```\n<div style=\"page-break-after: always;\"></div>\n```\n";
    let f = Fixture::with("s027", "\"all_notes\": true", &[("N.md", note)]);
    let b = body(&page(&f.export(), "n.md"));
    for kept in ["==plain==", "<mark style=\"background: #FFFF00;\">yellow</mark>", "<span style=\"color: #FF0000;\">red</span>", "<u>u</u>", "H<sub>2</sub>O", "x<sup>2</sup>"] {
        assert!(b.contains(kept), "{kept} is lost: {b}");
    }
    assert_eq!(b.matches("page-break").count(), 1, "only the one in the code block stays: {b}");
    assert!(b.contains("<div align=\"center\">\n\nCentred **text**\n\n</div>"), "kept, with blank lines for the Markdown inside: {b}");
}

// spec: SITE-028
#[test]
fn site_028_only_what_shale_reads_as_math_is_math_and_a_lone_dollar_is_text() {
    let note = "It costs $5 and $10.\n\nMath $x^2$ and $$y$$ and\n\n$$\nz\n$$\n\n```math\nw\n```\n\n`$a$` and \\$kept and [[N|$5 alias]].\n\n```\n$code$ $\n```\n";
    let f = Fixture::with("s028", "\"all_notes\": true", &[("N.md", note)]);
    let b = body(&page(&f.export(), "n.md"));
    assert!(b.contains("It costs \\$5 and \\$10.\n\nMath $x^2$ and $$y$$ and\n\n$$\nz\n$$\n\n```math\nw\n```"), "{b}");
    assert!(b.contains("`$a$` and \\$kept and [\\$5 alias](/n/)."), "{b}");
    assert!(b.contains("```\n$code$ $\n```"), "code is as written: {b}");
}

// spec: SITE-070
#[test]
fn site_070_text_hugo_would_read_as_a_shortcode_is_kept_as_text() {
    let f = Fixture::with("s070", "\"all_notes\": true", &[("N.md", "Use {{< youtube abc >}} and {{% note %}}x{{% /note %}} and\n\n```\n{{< in code >}}\n```\n\n{{< broken")]);
    let b = body(&page(&f.export(), "n.md"));
    assert!(b.contains("{{</* youtube abc */>}}") && b.contains("{{%/* note */%}}x{{%/* /note */%}}") && b.contains("{{</* in code */>}}"), "{b}");
    assert!(!b.contains("{{< broken") && b.contains("&#123;&#123;< broken"), "an unfinished one is written as characters: {b}");
}

// spec: SITE-033, SITE-034
#[test]
fn site_033_a_picture_embed_shows_its_picture_with_its_width_and_alias() {
    let f = Fixture::with("s033", "\"all_notes\": true", &[("N.md", "![[x.png]] ![[x.png|300]] ![[x.png|Sea at dusk|450]] ![[x.png|Only alt]] ![](x.png) ![Alt text](img/y.png) ![[X.PNG|300x200]]\n")]);
    f.bytes("x.png", b"png");
    f.bytes("img/y.png", b"png");
    let e = f.export();
    let b = body(&page(&e, "n.md"));
    assert!(b.starts_with("![](/media/files/x.png) ![](/media/files/x.png \"shale:width=300\") ![Sea at dusk](/media/files/x.png \"shale:width=450\") ![Only alt](/media/files/x.png) ![](/media/files/x.png) ![Alt text](/media/files/img/y.png) ![](/media/files/x.png \"shale:width=300\")"), "{b}");
    assert_eq!(e.media, ["static/media/files/img/y.png", "static/media/files/x.png"]);
    assert!(matches!(&e.files.iter().find(|f| f.path == "static/media/files/x.png").unwrap().content, Content::Copy(p) if p.ends_with("x.png")));
}

// spec: SITE-035
#[test]
fn site_035_audio_video_and_pdf_embeds_are_a_link_to_the_file_in_phase_1() {
    let f = Fixture::with("s035", "\"all_notes\": true", &[("N.md", "![[song.mp3]] ![[clip.mp4|The clip]] ![[doc.pdf]] ![](doc.pdf)\n")]);
    for n in ["song.mp3", "clip.mp4", "doc.pdf"] {
        f.bytes(n, b"data");
    }
    let e = f.export();
    let b = body(&page(&e, "n.md"));
    assert_eq!(b.trim(), "[song.mp3](/media/files/song.mp3) [The clip](/media/files/clip.mp4) [doc.pdf](/media/files/doc.pdf) [doc.pdf](/media/files/doc.pdf)");
    assert_eq!(e.media.len(), 3);
    assert_eq!(e.links, 4);
}

// spec: SITE-036
#[test]
fn site_036_off_vault_media_are_copied_when_they_resolve_here_and_a_placeholder_when_not() {
    let f = Fixture::with("s036", "\"all_notes\": true", &[("N.md", "![[@Media/a/pic.png|200]] ![[@Media/a/song.mp3]] ![[@Gone/x.png]] ![[@Media/missing.png]] ![Local](<file:///tmp/shale-site-nowhere/My Photo.png>) ![](<FILE_LINK>)\n")]);
    let own = f.root.join("elsewhere");
    std::fs::create_dir_all(&own).unwrap();
    std::fs::write(own.join("My Own.png"), b"a file link").unwrap();
    let n = std::fs::read_to_string(f.dir.join("N.md")).unwrap().replace("FILE_LINK", &format!("file://{}", own.join("My Own.png").display()));
    f.file("N.md", &n);
    let lib = f.root.join("library");
    std::fs::create_dir_all(lib.join("a")).unwrap();
    std::fs::write(lib.join("a/pic.png"), b"outside picture").unwrap();
    std::fs::write(lib.join("a/song.mp3"), b"outside song").unwrap();
    let e = f.export_with(&[Library { name: "Media".into(), path: lib.clone() }]);
    let b = body(&page(&e, "n.md"));
    let pic = e.media.iter().find(|m| m.ends_with("/pic.png")).expect("the picture is copied");
    assert!(pic.starts_with("static/media/outside/") && !pic.contains("library"), "a stable path that says nothing of this computer: {pic}");
    assert!(b.contains(&format!("![]({} \"shale:width=200\")", pic.replacen("static", "", 1))), "{b}");
    assert!(b.contains("](/media/outside/") && b.contains("song.mp3]"), "{b}");
    assert_eq!(e.media.len(), 3, "{:?}", e.media);
    assert!(e.media.iter().any(|m| m.ends_with("/My Own.png")), "a file link that resolves here is copied too: {:?}", e.media);
    assert_eq!(b.matches("*(media not available: ").count(), 3, "{b}");
    assert!(b.contains("media not available: x.png") && b.contains("media not available: missing.png") && b.contains("media not available: My Photo.png"));
    assert_eq!(e.warnings.len(), 3, "{:?}", e.warnings);
    assert!(e.warnings.iter().any(|w| w.contains("@Gone/x.png") && w.contains("not set up")));
    let all = everything(&e);
    assert!(!all.contains(lib.to_str().unwrap()) && !all.contains("shale-site-nowhere") && !all.contains("elsewhere"), "no path of this computer is written");
    // the same links on another device with the same library name give the same paths
    let other = f.export_with(&[Library { name: "media".into(), path: lib.clone() }]);
    assert_eq!(e.media, other.media);
}

// spec: SITE-038
#[test]
fn site_038_a_plugin_drawing_is_never_a_note_and_has_no_page_backlink_or_graph_entry() {
    let f =
        Fixture::with("s038", "\"all_notes\": true", &[("N.md", "![[Sketch.excalidraw]] [[Sketch.excalidraw.md]]\n"), ("Sketch.excalidraw.md", "---\nexcalidraw-plugin: parsed\n---\ndrawing words\n"), ("Board.canvas", "{\"nodes\":[]}")]);
    let e = f.export();
    assert_eq!(written_notes(&e), ["N.md"]);
    assert!(!paths(&e).iter().any(|p| p.to_lowercase().contains("sketch")));
    assert!(!everything(&e).contains("drawing words"));
    assert!(!text(&e, "data/graph.json").to_lowercase().contains("sketch"));
}

// spec: SITE-039, SITE-040
#[test]
fn site_039_backlinks_and_the_graph_hold_the_published_notes_only() {
    let f = Fixture::with("s039", "\"include_tags\": [\"pub\"]", &[("A.md", "#pub\n[[B]] [[B]] [[Hidden]]\n"), ("B.md", "#pub\n[[A]] [[B]] [[#x]]\n"), ("C.md", "#pub\n[[B|see]]\n"), ("Hidden.md", "[[B]] [[A]]\n")]);
    let e = f.export();
    let back: Value = serde_json::from_str(&text(&e, "data/backlinks.json")).unwrap();
    assert_eq!(back["b.md"], serde_json::json!([{"title": "A", "url": "/a/"}, {"title": "C", "url": "/c/"}]), "titles and URLs, every linker once, never the note itself");
    assert_eq!(back["a.md"], serde_json::json!([{"title": "B", "url": "/b/"}]));
    assert!(back.get("c.md").is_none() && back.get("hidden.md").is_none());
    let graph: Value = serde_json::from_str(&text(&e, "data/graph.json")).unwrap();
    let nodes: Vec<&str> = graph["nodes"].as_array().unwrap().iter().map(|n| n["url"].as_str().unwrap()).collect();
    assert_eq!(nodes, ["/a/", "/b/", "/c/"]);
    let links: Vec<(&str, &str)> = graph["links"].as_array().unwrap().iter().map(|l| (l["source"].as_str().unwrap(), l["target"].as_str().unwrap())).collect();
    assert_eq!(links, [("/a/", "/b/"), ("/b/", "/a/"), ("/c/", "/b/")]);
}

// spec: SITE-041, SITE-043
#[test]
fn site_041_folders_become_sections_and_a_folder_note_is_the_sections_text() {
    let f = Fixture::with(
        "s041",
        "\"all_notes\": true",
        &[
            ("Blog/index.md", "Welcome to the blog.\n"),
            ("Blog/First.md", "x\n"),
            ("Blog/Deep/Second.md", "x\n"),
            ("Wiki/Wiki.md", "---\ntitle: The wiki\n---\nWiki text\n"),
            ("Wiki/Page.md", "x\n"),
            ("Loose/One.md", "x\n"),
            ("index.md", "Home text\n"),
            ("Empty/Nothing.txt", "no notes\n"),
        ],
    );
    let e = f.export();
    let blog = page(&e, "blog/_index.md");
    assert_eq!(front(&blog)["title"], "Blog");
    assert!(body(&blog).contains("Welcome to the blog.") && !has(&e, "content/blog/index.md"), "the folder note is the text of the section, not a page");
    assert_eq!(front(&page(&e, "wiki/_index.md"))["title"], "The wiki", "its own title wins");
    assert_eq!(body(&page(&e, "loose/_index.md")).trim(), "", "a folder without a folder note is a section with no text");
    assert_eq!(front(&page(&e, "loose/_index.md"))["title"], "Loose");
    assert!(has(&e, "content/blog/deep/_index.md") && has(&e, "content/blog/deep/second.md"));
    assert!(!paths(&e).iter().any(|p| p.contains("empty") || p.contains("nothing")), "a folder with no published note is no section");
    let home = page(&e, "_index.md");
    assert!(body(&home).contains("Home text"), "the index page is the root folder note and the theme lists the sections and the latest notes");
    assert_eq!(front(&home)["title"], f.dir.file_name().unwrap().to_str().unwrap());
    // a link to a folder note is a link to its section
    f.file("Other.md", "[[/index]] [[Wiki]]\n");
    assert_eq!(body(&page(&f.export(), "other.md")).trim(), "[vault](/) [The wiki](/wiki/)");
}

// spec: SITE-046, SITE-047, SITE-070
#[test]
fn site_046_every_tree_has_the_config_and_the_vendored_theme_of_a_complete_site() {
    let f = Fixture::with("s046", "\"all_notes\": true", &[("N.md", "x\n")]);
    let e = f.export();
    let toml: toml::Table = text(&e, "hugo.toml").parse().unwrap();
    assert_eq!(toml["theme"].as_str(), Some("shale"));
    assert_eq!(toml["title"].as_str(), Some("vault"));
    let gm = &toml["markup"]["goldmark"];
    assert_eq!(gm["renderer"]["unsafe"].as_bool(), Some(true), "unsafe HTML is on");
    assert_eq!(gm["extensions"]["extras"]["subscript"]["enable"].as_bool(), Some(false), "`~~` is strike-through");
    assert_eq!(gm["extensions"]["extras"]["mark"]["enable"].as_bool(), Some(true));
    assert_eq!(gm["extensions"]["extras"]["superscript"]["enable"].as_bool(), Some(true));
    assert_eq!(gm["extensions"]["passthrough"]["enable"].as_bool(), Some(true), "math passes through");
    for path in super::theme::paths() {
        assert!(has(&e, path), "{path} is not in the tree");
    }
    assert!(text(&e, "themes/shale/layouts/_default/_markup/render-passthrough.html").contains("transform.ToMath"));
    assert!(text(&e, "themes/shale/layouts/partials/search.html").contains("pagefind-ui.js"));
    assert!(text(&e, "themes/shale/layouts/partials/backlinks.html").contains("hugo.Data.backlinks"));
    // every file below assets/site is in the tree
    fn files(dir: &std::path::Path, out: &mut Vec<String>, base: &std::path::Path) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                files(&p, out, base);
            } else {
                out.push(p.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/site");
    let mut sources = Vec::new();
    files(&base, &mut sources, &base);
    let mut tree: Vec<String> = super::theme::paths().into_iter().map(|p| p.strip_prefix("themes/shale/").map_or(p.to_string(), |r| format!("theme/{r}"))).collect();
    sources.sort();
    tree.sort();
    assert_eq!(sources, tree, "the files below assets/site/ and the theme the program carries are the same");
}
