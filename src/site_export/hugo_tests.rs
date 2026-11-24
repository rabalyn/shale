//! A real build (SITE-020, SITE-027, SITE-028, SITE-033, SITE-034, SITE-042, SITE-046…SITE-048,
//! SITE-070): the tree of an export is built by `hugo` and the HTML is checked. Hugo is an external
//! program (BUILD-018): a test skips with a printed note when it is missing or older than the
//! theme's `min_version`, and CI installs it, so it runs there.

use super::anchor::anchors;
use super::cli::{export, Args};
use super::test_support::Fixture;
use std::path::{Path, PathBuf};

/// The oldest Hugo the theme builds with (`themes/shale/theme.toml`, `min_version`).
const MIN: (u32, u32) = (0, 158);

/// Whether a usable Hugo is installed; says so when not (BUILD-018).
fn hugo_here() -> bool {
    let version = crate::program::command("hugo").arg("version").output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).into_owned());
    let parsed = version.as_deref().and_then(|v| v.split_whitespace().nth(1)?.strip_prefix('v')).and_then(|v| {
        let mut p = v.split('.').map(|n| n.split(['-', '+']).next().unwrap_or("").parse::<u32>().ok());
        Some((p.next()??, p.next()??))
    });
    match parsed {
        Some(v) if v >= MIN => true,
        Some(v) => {
            println!("skipping: hugo {}.{} is older than the theme's {}.{}", v.0, v.1, MIN.0, MIN.1);
            false
        }
        None => {
            println!("skipping: hugo is not installed");
            false
        }
    }
}

/// Whether Pagefind is installed (`scripts/install-pagefind.sh`); says so when not (BUILD-018).
fn pagefind_here() -> bool {
    let found = crate::program::command("pagefind").arg("--version").output().ok().is_some_and(|o| o.status.success());
    if !found {
        println!("skipping: pagefind is not installed (scripts/install-pagefind.sh)");
    }
    found
}

/// The bytes of a file of Pagefind's index: compressed, then its own `pagefind_dcd` marker.
fn unpack(path: &Path) -> Vec<u8> {
    let bytes = std::fs::read(path).unwrap();
    assert_eq!(bytes[..3], [0x1f, 0x8b, 0x08], "{} is compressed", path.display());
    miniz_oxide::inflate::decompress_to_vec(&bytes[10..bytes.len() - 8]).unwrap_or_else(|e| panic!("{}: {e:?}", path.display()))
}

/// Export `f` to its `out/` and build it; the folder `public/` and Hugo's messages.
fn build(f: &Fixture) -> (PathBuf, String) {
    let args = Args { vault: f.dir.clone(), out: f.out(), dry_run: false };
    export(&args, &[], &mut Vec::new(), &mut Vec::new()).unwrap();
    let public = f.root.join("public");
    let run = crate::program::command("hugo").arg("--source").arg(f.out()).arg("--destination").arg(&public).arg("--noBuildLock").env("HUGO_CACHEDIR", f.root.join("hugo-cache")).output().unwrap();
    let said = format!("{}{}", String::from_utf8_lossy(&run.stdout), String::from_utf8_lossy(&run.stderr));
    assert!(run.status.success(), "hugo failed:\n{said}");
    assert!(!said.contains("ERROR") && !said.contains("WARN"), "hugo has something to say:\n{said}");
    (public, said)
}

fn html(public: &Path, page: &str) -> String {
    std::fs::read_to_string(public.join(page).join("index.html")).unwrap_or_else(|e| panic!("{page}: {e}"))
}

fn all_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            all_files(&p, out);
        } else {
            out.push(p);
        }
    }
}

fn unescape_url(s: &str) -> String {
    let b = s.as_bytes();
    let (mut out, mut i) = (Vec::new(), 0);
    while i < b.len() {
        match (b[i], s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok())) {
            (b'%', Some(v)) => {
                out.push(v);
                i += 3;
            }
            (c, _) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The values of an attribute (`href`, `id`) in a page, in order.
fn attrs(html: &str, name: &str) -> Vec<String> {
    let pat = format!(" {name}=\"");
    html.match_indices(&pat).filter_map(|(at, _)| html[at + pat.len()..].split('"').next().map(String::from)).collect()
}

const TARGET: &str = "# First part\n\n## Plan **v2** (draft)? Ünï: a_b\n\n## Same\n\n## Same\n\n## A  B\n\n## C++ & D\n\n## 1. First\n\n## [[Plan]] and `code`\n\n## ½ ² ٣\n\n## Emoji 😀 ok\n\nTarget text. Back to [[Plan]].\n";

fn site() -> Fixture {
    let f = Fixture::with(
        "hugo",
        "\"include_tags\": [\"pub\"]",
        &[
            ("index.md", "#pub\nWelcome to the home page.\n"),
            (
                "Plan.md",
                "---\ntags: [alpha]\ntype: recipe\n---\n#pub #beta\nLinks: [[Target#Plan v2 (draft)? Ünï: a_b|one]], [[Target#Same]], [[Target#Emoji 😀 ok]], [[Target#½ ² ٣]], [[Target#1. First]], [[Sub/Page]], [[Ünï Note]], [[Hidden note|a friend]].\n\n\
                 Marks: ==plain== <mark style=\"background: #FFFF00;\">yellow</mark> <span style=\"color: #FF0000;\">red</span> ~~struck~~ x<sup>2</sup> and costs $5 and $10, shortcode {{< youtube abc >}}.\n\n\
                 Math $x^2_1$ and\n\n$$\n\\frac{a}{b}\n$$\n\n```math\nE = mc^2\n```\n\n\
                 ![[pic.png|Sea at dusk|300]] ![[pic.png]]\n\n- [x] done\n- [ ] open\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\nA footnote[^1].\n\n[^1]: The note.\n\n```rust\nfn main() {}\n```\n\n<div align=\"center\">\nCentred **text**\n</div>\n\n<div style=\"page-break-after: always;\"></div>\n",
            ),
            ("Target.md", &format!("#pub\n{TARGET}")),
            ("Sub/Page.md", "#pub\nSee [[Plan]] and [[Target]].\n"),
            ("Ünï Note.md", "#pub\nUnicode name.\n"),
            ("Lonely.md", "#pub\nNobody links here.\n"),
            ("Hidden note.md", "Hidden words about a hidden-topic, see [[Plan]].\n"),
        ],
    );
    f.bytes("pic.png", &std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/icon/shale-256.png")).unwrap());
    f
}

// spec: SITE-020, SITE-021, SITE-022, SITE-025, SITE-027, SITE-028, SITE-029, SITE-030, SITE-031, SITE-032, SITE-033, SITE-034, SITE-046, SITE-047, SITE-070
#[test]
fn site_070_the_exported_tree_builds_and_the_pages_show_what_the_notes_say() {
    if !hugo_here() {
        return;
    }
    let f = site();
    let (public, _) = build(&f);
    let plan = html(&public, "plan");
    // math is typeset at build time, with no script (SITE-028); a lone dollar is text
    assert!(plan.contains("<math") && plan.contains("display=\"block\""), "{plan}");
    assert!(plan.matches("<math").count() >= 3, "inline, display and a math fence");
    assert!(plan.contains("costs $5 and $10"), "{plan}");
    assert!(!plan.contains("katex.min.js"));
    // marks, colours, strike-through, superscript (SITE-027)
    for shown in ["<mark>plain</mark>", "<mark style=\"background: #FFFF00;\">yellow</mark>", "<span style=\"color: #FF0000;\">red</span>", "<del>struck</del>", "x<sup>2</sup>"] {
        assert!(plan.contains(shown), "{shown} is missing:\n{plan}");
    }
    // footnotes, tables, task lists (a box that cannot be changed), code with its language (SITE-029, SITE-030)
    assert!(plan.contains("class=\"footnotes\"") && plan.contains("<table>") && plan.contains("<input checked=\"\" disabled=\"\" type=\"checkbox\">") && plan.contains("<input disabled=\"\" type=\"checkbox\">"));
    assert!(plan.contains("class=\"highlight\"") || plan.contains("chroma"), "{plan}");
    // the page break leaves no trace; the aligned block is kept (SITE-031, SITE-032)
    assert!(!plan.contains("page-break") && plan.contains("<div align=\"center\">") && plan.contains("<p>Centred <strong>text</strong></p>"), "{plan}");
    // a picture at its width with its alias as alt text; one without (SITE-033, SITE-034)
    assert!(plan.contains("<img src=\"/media/files/pic.png\" alt=\"Sea at dusk\" width=\"300\" loading=\"lazy\">"), "{plan}");
    assert!(plan.contains("<img src=\"/media/files/pic.png\" alt=\"\" loading=\"lazy\">"));
    assert!(public.join("media/files/pic.png").is_file());
    // a link is followed by the text that follows it, with nothing of the hook between
    assert!(plan.contains("a_b\">one</a>, <a href=\"/target/#same\">"), "{plan}");
    // the shortcode text is text
    assert!(plan.contains("{{&lt; youtube abc &gt;}}"), "{plan}");
    // the type goes to the page, where the theme reads it; the tags are pages
    assert!(plan.contains("data-type=\"recipe\"") && plan.contains("href=\"/tags/alpha/\""));
    assert!(public.join("tags/alpha/index.html").is_file() && public.join("tags/beta/index.html").is_file());
    // the title, the unicode page
    assert!(plan.contains("<title>Plan – vault</title>") && plan.contains("<h1>Plan</h1>"));
    assert!(html(&public, "ünï-note").contains("Unicode name."), "{:?}", std::fs::read_dir(&public).unwrap().flatten().map(|e| e.file_name()).collect::<Vec<_>>());
    assert!(plan.contains("href=\"/%C3%BCn%C3%AF-note/\">Ünï Note</a>"), "{plan}");
}

// spec: SITE-020
#[test]
fn site_020_every_heading_link_lands_on_an_anchor_that_hugo_made() {
    if !hugo_here() {
        return;
    }
    let f = site();
    let (public, _) = build(&f);
    // the ids Hugo made are the ones the exporter predicted, headings with punctuation, duplicates and unicode included
    let target = html(&public, "target");
    // the headings with an id, in order (the page's own title is an h1 without one)
    let made: Vec<String> = target
        .match_indices("<h")
        .filter(|(at, _)| target[*at + 2..].starts_with(|c: char| ('1'..='6').contains(&c)))
        .filter_map(|(at, _)| {
            let tag_end = target[at..].find('>')?;
            attrs(&target[at..at + tag_end], "id").into_iter().next()
        })
        .collect();
    let predicted: Vec<String> = anchors(TARGET).into_iter().map(|a| a.id).collect();
    assert_eq!(made, predicted, "Hugo's ids and the exporter's: {target}");
    assert!(target.contains("id=\"first-part\""), "{target}");
    // and every link with a fragment, in every page, points at an id of its target page
    let mut checked = 0;
    let mut files = Vec::new();
    all_files(&public, &mut files);
    for file in files.iter().filter(|p| p.extension().is_some_and(|e| e == "html")) {
        let page = std::fs::read_to_string(file).unwrap();
        for href in attrs(&page, "href").into_iter().filter(|h| h.starts_with('/') && h.contains('#')) {
            let (path, frag) = href.split_once('#').unwrap();
            let want = unescape_url(frag);
            let there = std::fs::read_to_string(public.join(unescape_url(path).trim_start_matches('/')).join("index.html")).unwrap();
            let ids: Vec<String> = attrs(&there, "id").iter().map(|i| unescape_url(i)).collect();
            assert!(ids.contains(&want), "{href} in {}: no id {want} in {ids:?}", file.display());
            checked += 1;
        }
    }
    assert!(checked >= 5, "the heading links are checked ({checked})");
}

// spec: SITE-039, SITE-041, SITE-042, SITE-043, SITE-010
#[test]
fn site_042_a_page_shows_its_backlinks_and_the_sections_and_the_index_list_the_notes() {
    if !hugo_here() {
        return;
    }
    let f = site();
    let (public, _) = build(&f);
    let target = html(&public, "target");
    assert!(target.contains("class=\"backlinks\""), "{target}");
    let panel = target.split("class=\"backlinks\"").nth(1).unwrap();
    assert!(panel.contains("href=\"/plan/\">Plan</a>") && panel.contains("href=\"/sub/page/\">Page</a>"), "{panel}");
    assert!(!panel.contains("hidden"), "an unpublished note is no backlink");
    assert!(!html(&public, "lonely").contains("class=\"backlinks\""), "a page with none shows no panel");
    // the section's index and the index page (SITE-041, SITE-043)
    let sub = html(&public, "sub");
    assert!(sub.contains("<h1>Sub</h1>") && sub.contains("href=\"/sub/page/\">Page</a>"), "{sub}");
    let home = public.join("index.html");
    let home = std::fs::read_to_string(home).unwrap();
    assert!(home.contains("Welcome to the home page.") && home.contains("href=\"/sub/\">Sub</a>") && home.contains("Recently changed") && home.contains("href=\"/lonely/\">Lonely</a>"), "{home}");
    // nothing of the unpublished note is anywhere in what hugo built
    let mut files = Vec::new();
    all_files(&public, &mut files);
    for file in files.iter().filter(|p| p.extension().is_some_and(|e| ["html", "json", "xml", "txt"].contains(&e.to_str().unwrap_or("")))) {
        let text = std::fs::read_to_string(file).unwrap().to_lowercase();
        assert!(!text.contains("hidden-topic") && !text.contains("hidden words") && !text.contains("hidden note"), "{}", file.display());
    }
}

// spec: SITE-048
#[test]
fn site_048_the_pages_have_the_search_box_for_pagefind_and_only_the_notes_are_indexed() {
    if !hugo_here() {
        return;
    }
    let f = site();
    let (public, _) = build(&f);
    let plan = html(&public, "plan");
    assert!(plan.contains("<div class=\"search\" id=\"search\" role=\"search\">"), "{plan}");
    assert!(plan.contains("src=\"/pagefind/pagefind-ui.js\"") && plan.contains("href=\"/pagefind/pagefind-ui.css\"") && plan.contains("new PagefindUI({ element: \"#search\""));
    // Pagefind indexes what is inside `data-pagefind-body`: the note, not the search box, the header or the backlinks
    assert_eq!(plan.matches("data-pagefind-body").count(), 1);
    let body = plan.split("data-pagefind-body").nth(1).unwrap().split("</article>").next().unwrap();
    assert!(body.contains("Marks:") && !body.contains("Linked from") && !body.contains("id=\"search\""));
    // nothing is loaded from another site
    assert!(!plan.contains("src=\"http") && !plan.contains("href=\"http"), "{plan}");
}

// spec: SITE-048
#[test]
fn site_048_pagefind_finds_the_words_of_a_published_note_and_no_word_of_an_unpublished_one() {
    if !hugo_here() || !pagefind_here() {
        return;
    }
    let f = Fixture::with("s048p", "\"all_notes\": true", &[("Harbour.md", "Zebrafish swim in the harbour.\n"), ("Closed.md", "---\npublish: false\n---\nQuokka secrets of the closed note.\n")]);
    let (public, _) = build(&f);
    let run = crate::program::command("pagefind").arg("--site").arg(&public).output().unwrap();
    assert!(run.status.success(), "pagefind failed:\n{}{}", String::from_utf8_lossy(&run.stdout), String::from_utf8_lossy(&run.stderr));
    let index = public.join("pagefind");
    let mut parts = Vec::new();
    for dir in ["fragment", "index"] {
        for e in std::fs::read_dir(index.join(dir)).unwrap().flatten() {
            parts.push((dir, unpack(&e.path())));
        }
    }
    let text = |dir: &str| parts.iter().filter(|(d, _)| *d == dir).map(|(_, b)| String::from_utf8_lossy(b).into_owned()).collect::<Vec<_>>().join("\n");
    // the page of the note is in the index with its words, and its word is a word of the search index
    let (fragments, words) = (text("fragment"), text("index"));
    assert!(fragments.contains("\"url\":\"/harbour/\"") && fragments.contains("Zebrafish swim in the harbour."), "{fragments}");
    assert!(words.contains("zebrafish"), "the word is in the index");
    // nothing of the note that is not published is
    for gone in ["quokka", "closed", "secrets"] {
        assert!(!fragments.to_lowercase().contains(gone) && !words.contains(gone), "{gone} is in the index");
    }
    // the header and the backlinks are not indexed: only what is in the page's body
    assert!(!fragments.contains("Linked from"), "{fragments}");
}

// spec: SITE-001
#[test]
fn site_001_the_hello_world_page_is_the_index_of_a_site_without_a_note() {
    if !hugo_here() {
        return;
    }
    let f = Fixture::with("s001h", "\"include_tags\": [\"public\"]", &[("Plan.md", "Nothing is tagged.\n")]);
    let (public, _) = build(&f);
    let index = html(&public, "");
    assert!(index.contains("Hello from Shale") && index.contains("This page was published with Shale.") && index.contains("No notes are chosen for publishing yet."), "{index}");
    assert!(!index.contains("Plan") && !index.contains("Folders") && !index.contains("Recently changed"), "{index}");
}
