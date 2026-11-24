//! A real build of what phase 2 adds (SITE-035, SITE-037, SITE-040, SITE-044): players, drawings,
//! the graph page and the layouts per type, looked at in the HTML Hugo makes. Hugo is an external
//! program (BUILD-018): a test skips with a printed note when it is missing.

use super::hugo_tests::{all_files, attrs, build, html, hugo_here};
use super::test_support::Fixture;
use crate::drawing::{excalidraw_md, Scene};

fn unescape(s: &str) -> String {
    s.replace("&#34;", "\"").replace("&#39;", "'").replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
}

// spec: SITE-035, SITE-034
#[test]
fn site_035_the_pages_show_a_player_for_a_sound_a_video_and_a_pdf_with_a_link_inside() {
    if !hugo_here() {
        return;
    }
    let f = Fixture::with("h035", "\"all_notes\": true", &[("Sounds.md", "![[song.mp3]]\n\n![[clip.mp4|The clip|480]]\n\n![[doc.pdf|300]] ![[doc.pdf]]\n\n![[pic.png|120]]\n\n[[song.mp3|just a link]]\n")]);
    for n in ["song.mp3", "clip.mp4", "doc.pdf", "pic.png"] {
        f.bytes(n, b"data");
    }
    let (public, _) = build(&f);
    let page = html(&public, "sounds");
    assert!(page.contains("<audio class=\"player\" controls preload=\"none\" src=\"/media/files/song.mp3\"><a href=\"/media/files/song.mp3\">song.mp3</a></audio>"), "{page}");
    assert!(page.contains("<video class=\"player\" controls preload=\"metadata\" src=\"/media/files/clip.mp4\" width=\"480\"><a href=\"/media/files/clip.mp4\">The clip</a></video>"), "the width of SITE-034, the alias for the link: {page}");
    assert!(page.contains("<object class=\"player pdf\" data=\"/media/files/doc.pdf\" type=\"application/pdf\" style=\"width: 300px\"><a href=\"/media/files/doc.pdf\" download>doc.pdf</a></object>"), "{page}");
    assert_eq!(page.matches("<object class=\"player pdf\"").count(), 2);
    assert!(page.contains("<img src=\"/media/files/pic.png\" alt=\"\" width=\"120\" loading=\"lazy\">"), "a picture stays a picture: {page}");
    assert!(page.contains("<a href=\"/media/files/song.mp3\">just a link</a>") && page.matches("<audio").count() == 1, "a link is a link: {page}");
    for n in ["song.mp3", "clip.mp4", "doc.pdf"] {
        assert!(public.join("media/files").join(n).is_file(), "{n} is published");
    }
    assert!(!page.contains("src=\"http") && !page.contains("<iframe"), "nothing from another site");
}

// spec: SITE-037, SITE-007, SITE-006
#[test]
fn site_037_a_drawing_is_a_picture_on_the_page_and_its_svg_is_in_the_site() {
    if !hugo_here() {
        return;
    }
    let map = r##"{"type":"excalidraw","version":2,"elements":[
{"id":"r1","type":"rectangle","x":0,"y":0,"width":200,"height":100,"strokeColor":"#1971c2","backgroundColor":"#a5d8ff","fillStyle":"hachure","roughness":1,"seed":7,"link":"[[Public]]"},
{"id":"t1","type":"text","x":10,"y":30,"width":400,"height":30,"text":"A public note and [[Hidden|a friend]]","fontSize":20,"fontFamily":1,"containerId":"r1","textAlign":"center"}
],"appState":{"viewBackgroundColor":"#ffffff"},"files":{}}"##;
    let f = Fixture::with(
        "h037",
        "\"include_tags\": [\"pub\"]",
        &[
            ("Drawn.md", "#pub\n![[Map.excalidraw|Our map]]\n\n![[Plugin.excalidraw.md|260]]\n"),
            ("Public.md", "#pub\nA public note.\n"),
            ("Hidden.md", "Hidden words.\n"),
            ("Map.excalidraw", map),
            ("Plugin.excalidraw.md", &excalidraw_md::new_file(&Scene::parse(map).unwrap())),
        ],
    );
    let (public, _) = build(&f);
    let page = html(&public, "drawn");
    assert!(page.contains("<object class=\"drawing\" data=\"/media/drawings/Map.excalidraw.svg\" type=\"image/svg+xml\" width=\""), "{page}");
    assert!(page.contains("<img src=\"/media/drawings/Map.excalidraw.svg\" alt=\"Our map\" loading=\"lazy\"></object>"), "the picture itself when the browser has no object: {page}");
    assert!(page.contains("<object class=\"drawing\" data=\"/media/drawings/Plugin.excalidraw.md.svg\" type=\"image/svg+xml\" width=\"260\" height=\""), "the width of SITE-034: {page}");
    let svg = std::fs::read_to_string(public.join("media/drawings/Map.excalidraw.svg")).unwrap();
    assert!(svg.starts_with("<svg ") && svg.contains("<a href=\"../../public/\" target=\"_top\">") && svg.contains("A public note and a friend"), "{svg}");
    assert!(!svg.contains("idden") && !page.contains("idden"), "nothing of the note that is not published");
    // the width and the height of the object keep the picture's proportions
    let (w, h): (f32, f32) = {
        let tag = page.split("Plugin.excalidraw.md.svg").nth(1).unwrap();
        let get = |k: &str| tag.split(&format!("{k}=\"")).nth(1).unwrap().split('"').next().unwrap().parse().unwrap();
        (get("width"), get("height"))
    };
    let view: Vec<f32> = svg.split("viewBox=\"0 0 ").nth(1).unwrap().split('"').next().unwrap().split(' ').map(|n| n.parse().unwrap()).collect();
    assert!((w / h - view[0] / view[1]).abs() < 0.05, "{w}x{h} and {view:?}");
}

// spec: SITE-040
#[test]
fn site_040_the_graph_page_has_the_published_notes_and_their_links_and_loads_nothing() {
    if !hugo_here() {
        return;
    }
    let f = Fixture::with(
        "h040",
        "\"include_tags\": [\"pub\"]",
        &[
            ("Alpha.md", "#pub\nTo [[Beta]] and [[Hidden]].\n"),
            ("Beta.md", "#pub\nBack to [[Alpha]].\n"),
            ("Graph.md", "#pub\nA note called Graph.\n"),
            ("Odd.md", "---\ntitle: \"Tricky </script> & \\\"quoted\\\" title\"\n---\n#pub\nx\n"),
            ("Hidden.md", "Hidden words, see [[Alpha]].\n"),
        ],
    );
    let (public, said) = build(&f);
    assert!(!said.contains("WARN"), "{said}");
    let graph = std::fs::read_to_string(public.join("graph.html")).expect("the graph page is at /graph.html");
    assert!(html(&public, "graph").contains("A note called Graph."), "a note called Graph has its own page, /graph/");
    // the page carries its data: the published notes and the links between them, and nothing else
    let data = unescape(&attrs(&graph, "data-graph")[0]);
    let json: serde_json::Value = serde_json::from_str(&data).unwrap_or_else(|e| panic!("{e}: {data}"));
    let titles: Vec<&str> = json["nodes"].as_array().unwrap().iter().map(|n| n["title"].as_str().unwrap()).collect();
    assert_eq!(titles, ["Alpha", "Beta", "Graph", "Tricky </script> & \"quoted\" title"]);
    let links: Vec<(&str, &str)> = json["links"].as_array().unwrap().iter().map(|l| (l["source"].as_str().unwrap(), l["target"].as_str().unwrap())).collect();
    assert_eq!(links, [("/alpha/", "/beta/"), ("/beta/", "/alpha/")]);
    assert!(!graph.contains("idden") && !graph.to_lowercase().contains("hidden"), "{graph}");
    // the title cannot end the page's script or attribute
    assert!(!graph.contains("Tricky </script>") && graph.contains("Tricky \\u003c/script\\u003e"), "{graph}");
    // the script is the theme's own, written to the site, and the page loads nothing from another site
    assert!(graph.contains("<script src=\"/shale-graph.js\"></script>") && public.join("shale-graph.js").is_file());
    assert!(!graph.contains("src=\"http") && !graph.contains("href=\"http") && !graph.contains("//cdn"), "{graph}");
    let js = std::fs::read_to_string(public.join("shale-graph.js")).unwrap();
    let js = js.replace("http://www.w3.org/2000/svg", "");
    assert!(!js.contains("http:") && !js.contains("https:") && !js.contains("import(") && !js.contains("fetch(") && !js.contains("XMLHttpRequest"), "the script reads the page and loads nothing");
    // without the script the page lists the notes, and every page links to the graph
    assert!(graph.contains("<noscript>") && graph.contains("href=\"/beta/\">Beta</a>"), "{graph}");
    assert!(html(&public, "alpha").contains("href=\"/graph.html\">Graph</a>"));
    // a site's other pages are as they were: no page was made of the graph's data, and the search indexes the notes only
    assert!(!graph.contains("data-pagefind-body"), "the graph page is not indexed");
    let mut files = Vec::new();
    all_files(&public, &mut files);
    assert!(files.iter().filter(|p| p.extension().is_some_and(|e| e == "html")).count() >= 6);
}

fn typed() -> Fixture {
    Fixture::with(
        "h044",
        "\"all_notes\": true, \"private_types\": [\"email\", \"event\", \"post\", \"person\"]",
        &[
            (
                "Mail.md",
                "---\ntype: email\nsubject: \"Plans for May\"\nfrom: \"Ann <ann@example.org>\"\nto: \"Bob <bob@example.org>\"\ncc: \"carol@example.org\"\ndate: 2026-05-01 10:30\nmessage-id: \"<abc@example.org>\"\npeople:\n  - \"[[Ann]]\"\n  - \"[[Hidden friend]]\"\noriginal: \"[[z_media/secret.eml]]\"\n---\n# Plans for May\n\n## Message\n\nSee you.\n",
            ),
            (
                "Kickoff.md",
                "---\ntype: event\ntitle: \"Kickoff\"\nstart: 2026-05-02 09:00\nend: 2026-05-02 10:00\nlocation: \"Room 4\"\ncalendar: \"Work\"\nrepeats: weekly\norganizer: \"Ann <ann@example.org>\"\nattendees:\n  - \"Ann\"\n  - \"Bob\"\nstatus: confirmed\nurl: \"https://example.org/kickoff\"\n---\nAgenda.\n",
            ),
            ("Toot.md", "---\ntype: social-post\nnetwork: mastodon\nauthor: \"@ann\"\nauthor-name: \"Ann A.\"\nauthor-url: \"https://social.example/@ann\"\ndate: 2026-05-03 12:00\nurl: \"javascript:alert(1)\"\nthread: 3\n---\nA post.\n"),
            ("Post.md", "---\ntype: Post\nauthor: \"@bob\"\nurl: \"https://social.example/@bob/1\"\n---\nAnother post.\n"),
            ("People/Ann.md", "---\ntype: person\nemail: ann@example.org\norganization: example.org\nphone: \"+49 30 1234\"\nrole: Architect\n---\nAnn is an architect.\n"),
            ("Recipe.md", "---\ntype: recipe\nfrom: \"nobody@example.org\"\nstart: now\n---\nA plain page.\n"),
            ("Hidden friend.md", "---\npublish: false\n---\nHidden words.\n"),
        ],
    )
}

// spec: SITE-044, SITE-010
#[test]
fn site_044_notes_of_type_email_event_post_and_person_get_their_header_and_any_other_type_the_plain_page() {
    if !hugo_here() {
        return;
    }
    let f = typed();
    let (public, _) = build(&f);
    let mail = html(&public, "mail");
    assert!(mail.contains("<header class=\"note-header note-email\">") && mail.contains("<p class=\"kind\">E-mail</p>"), "{mail}");
    for want in [
        "<dt>From</dt>",
        "Ann &lt;ann@example.org&gt;",
        "<dt>To</dt>",
        "Bob &lt;bob@example.org&gt;",
        "<dt>Cc</dt>",
        "<dd>carol@example.org</dd>",
        "<dt>Date</dt>",
        "2026-05-01 10:30",
        "<dt>Subject</dt>",
        "Plans for May",
        "<dt>People</dt>",
        "<li>Ann</li>",
    ] {
        assert!(mail.contains(want), "{want}: {mail}");
    }
    assert!(!mail.contains("mailto:") && !mail.contains("abc@example.org") && !mail.contains("secret") && !mail.contains("idden"), "an address stays as written, and what is not in the note's header or not published is not shown: {mail}");
    assert!(mail.contains("data-type=\"email\""));
    // the header is above the text, which is still there
    assert!(mail.find("note-header").unwrap() < mail.find("See you.").unwrap());
    let event = html(&public, "kickoff");
    assert!(event.contains("<header class=\"note-header note-event\">") && event.contains("<p class=\"kind\">Event</p>"), "{event}");
    for want in [
        "<dt>Start</dt>",
        "2026-05-02 09:00",
        "<dt>End</dt>",
        "<dt>Repeats</dt>",
        "weekly",
        "<dt>Where</dt>",
        "Room 4",
        "<dt>Calendar</dt>",
        "Work",
        "<dt>Organizer</dt>",
        "<dt>Attendees</dt>",
        "<li>Ann</li><li>Bob</li>",
        "<dt>Status</dt>",
        "confirmed",
        "<a href=\"https://example.org/kickoff\" rel=\"noopener\">",
    ] {
        assert!(event.contains(want), "{want}: {event}");
    }
    assert!(!event.contains("<dt>All day</dt>"), "a field the note lacks is not shown: {event}");
    let toot = html(&public, "toot");
    assert!(toot.contains("<header class=\"note-header note-social-post\">") && toot.contains("<p class=\"kind\">Post</p>"), "{toot}");
    for want in ["<dt>Author</dt>", "Ann A.", "<dt>Handle</dt>", "@ann", "<a href=\"https://social.example/@ann\" rel=\"noopener\">", "<dt>Network</dt>", "mastodon", "<dt>Posted</dt>", "2026-05-03 12:00", "<dt>Thread</dt>"] {
        assert!(toot.contains(want), "{want}: {toot}");
    }
    assert!(toot.contains("<dt>Source</dt>") && toot.contains("javascript:alert(1)") && !toot.contains("href=\"javascript"), "only a web address is a link: {toot}");
    let post = html(&public, "post");
    assert!(post.contains("note-header note-post") && post.contains("<a href=\"https://social.example/@bob/1\" rel=\"noopener\">") && post.contains("data-type=\"Post\""), "the type is read in any case: {post}");
    let person = html(&public, "people/ann");
    assert!(person.contains("<header class=\"note-header note-person\">") && person.contains("<p class=\"kind\">Person</p>"), "{person}");
    for want in ["<dt>E-mail</dt>", "<dd>ann@example.org</dd>", "<dt>Phone</dt>", "&#43;49 30 1234", "<dt>Organization</dt>", "example.org", "<dt>Role</dt>", "Architect"] {
        assert!(person.contains(want), "{want}: {person}");
    }
    assert!(!person.contains("mailto:"));
    // any other type, and a note with none, is the plain page: its properties are not shown
    let plain = html(&public, "recipe");
    assert!(!plain.contains("note-header") && !plain.contains("nobody@example.org") && plain.contains("A plain page.") && plain.contains("data-type=\"recipe\""), "{plain}");
}

// spec: SITE-044
#[test]
fn site_044_the_header_shows_only_the_front_matter_of_the_published_note() {
    if !hugo_here() {
        return;
    }
    let f = Fixture::with(
        "h044p",
        "\"all_notes\": true, \"private_types\": [\"email\"]",
        &[("Mail.md", "---\ntype: email\nfrom: \"Ann <ann@example.org>\"\n---\nBody text.\n"), ("Other.md", "---\ntype: email\n---\nAn e-mail note without a header.\n")],
    );
    let (public, _) = build(&f);
    let mail = html(&public, "mail");
    assert!(mail.contains("<dt>From</dt>") && !mail.contains("<dt>To</dt>") && !mail.contains("<dt>Date</dt>"), "{mail}");
    let other = html(&public, "other");
    assert!(!other.contains("note-header") && other.contains("An e-mail note without a header.") && other.contains("data-type=\"email\""), "a typed note with no field has no header: {other}");
}
