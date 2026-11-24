//! Drawings in a site (SITE-037, SITE-038, SITE-006, SITE-007): what an export writes for a drawing
//! that a published note embeds, in each of the three formats.

use super::test_support::{everything, has, page, paths, text, Fixture};
use crate::drawing::{excalidraw_md, Scene};

const MAP: &str = r##"{"type":"excalidraw","version":2,"elements":[
{"id":"r1","type":"rectangle","x":0,"y":0,"width":200,"height":100,"strokeColor":"#1971c2","backgroundColor":"#a5d8ff","fillStyle":"solid","roughness":0,"seed":1,"link":"[[Public]]"},
{"id":"t1","type":"text","x":10,"y":30,"width":400,"height":30,"text":"See [[Public#Goals]] and [[Hidden|my friend]]","fontSize":20,"fontFamily":2,"containerId":"r1","textAlign":"center"},
{"id":"r2","type":"ellipse","x":300,"y":0,"width":100,"height":100,"seed":2,"link":"[[Hidden]]"},
{"id":"r3","type":"rectangle","x":0,"y":200,"width":50,"height":50,"seed":3,"link":"[[Public#Goals]]"},
{"id":"t2","type":"text","x":420,"y":40,"width":80,"height":20,"text":"https://example.org/x","fontSize":16,"fontFamily":3,"link":"https://example.org/x"}
],"appState":{"viewBackgroundColor":"#ffffff"},"files":{}}"##;

const BOARD: &str = r##"{"nodes":[
{"id":"a1","type":"text","text":"a card","x":0,"y":0,"width":200,"height":60},
{"id":"c1","type":"file","file":"Public.md","x":0,"y":120,"width":200,"height":60},
{"id":"c2","type":"file","file":"Hidden.md","x":260,"y":120,"width":200,"height":60}
],"edges":[{"id":"e1","fromNode":"a1","toNode":"c1"}]}"##;

fn vault(tag: &str) -> Fixture {
    Fixture::with(
        tag,
        "\"include_tags\": [\"pub\"]",
        &[
            ("N.md", "#pub\n![[Map.excalidraw]]\n\n![[Plugin.excalidraw.md|The plan|300]] ![[Board.canvas]] ![[Empty.excalidraw]]\n"),
            ("Public.md", "#pub\n# Goals\nA public note.\n"),
            ("Hidden.md", "Hidden words. ![[Secret.excalidraw]]\n"),
            ("Board.canvas", BOARD),
            ("Plugin.excalidraw.md", &excalidraw_md::new_file(&Scene::parse(MAP).unwrap())),
            ("Map.excalidraw", MAP),
            ("Secret.excalidraw", &MAP.replace("See ", "Secret plans ")),
            ("Unused.excalidraw", MAP),
            ("Empty.excalidraw", "{\"type\":\"excalidraw\",\"version\":2,\"elements\":[]}"),
        ],
    )
}

// spec: SITE-037, SITE-006
#[test]
fn site_037_a_drawing_that_a_published_note_embeds_is_written_as_an_svg_picture() {
    let f = vault("s037");
    let e = f.export();
    let b = text(&e, "content/n.md");
    for want in ["![](/media/drawings/Map.excalidraw.svg \"shale:drawing=", "![The plan](/media/drawings/Plugin.excalidraw.md.svg \"shale:drawing=", ";width=300\")", "![](/media/drawings/Board.canvas.svg \"shale:drawing="] {
        assert!(b.contains(want), "{want} is not in the page:\n{b}");
    }
    for svg in ["static/media/drawings/Map.excalidraw.svg", "static/media/drawings/Plugin.excalidraw.md.svg", "static/media/drawings/Board.canvas.svg"] {
        assert!(e.media.contains(&svg.to_string()), "{svg} is not written: {:?}", e.media);
        let s = text(&e, svg);
        assert!(s.starts_with("<svg ") && s.contains("<text") && s.contains("viewBox"), "{svg}: {s}");
    }
    // the size in the embed is the size of the picture
    let map = text(&e, "static/media/drawings/Map.excalidraw.svg");
    let size = map.split("viewBox=\"0 0 ").nth(1).unwrap().split('"').next().unwrap().replace(' ', "x");
    assert!(b.contains(&format!("Map.excalidraw.svg \"shale:drawing={size}\"")), "{size}: {b}");
    // a drawing that no published note embeds is not written, nor is one that only an unpublished note embeds (SITE-006)
    assert!(!paths(&e).iter().any(|p| p.contains("Unused") || p.contains("Secret")), "{:?}", paths(&e));
    assert!(!everything(&e).contains("Secret plans"));
    // a drawing that cannot be drawn is a placeholder and a warning
    assert!(b.contains("*(media not available: Empty.excalidraw)*"), "{b}");
    assert!(e.warnings.iter().any(|w| w.contains("Empty.excalidraw") && w.contains("empty")), "{:?}", e.warnings);
    // the same drawing embedded twice is one file
    f.file("N.md", "#pub\n![[Map.excalidraw]] ![[Map.excalidraw|200]]\n");
    let again = f.export();
    assert_eq!(again.media, ["static/media/drawings/Map.excalidraw.svg"]);
    assert!(text(&again, "content/n.md").contains(";width=200\")"));
    // a link to a drawing is not an embed: it is its text, and the drawing is not written
    f.file("N.md", "#pub\n[[Map.excalidraw]] and [x](Map.excalidraw)\n");
    let linked = f.export();
    assert!(linked.media.is_empty() && !has(&linked, "static/media/drawings/Map.excalidraw.svg"));
}

// spec: SITE-037, SITE-007, SITE-082
#[test]
fn site_037_links_in_a_drawing_follow_the_rule_of_links_in_notes() {
    let f = vault("s037l");
    let e = f.export();
    let map = text(&e, "static/media/drawings/Map.excalidraw.svg");
    assert!(map.contains("<a href=\"../../public/\" target=\"_top\">") && map.contains("<a href=\"../../public/#goals\" target=\"_top\">"), "the link to a published note is a link, to its heading when it names one: {map}");
    assert!(map.contains("See Public &gt; Goals and my friend"), "{map}");
    assert!(map.contains("<a href=\"https://example.org/x\" target=\"_top\">"), "a web address is kept: {map}");
    // a note that is not published: its text, no address, no name beyond the text
    assert!(map.contains("my friend") && !map.contains("hidden") && !map.contains("Hidden") && !map.contains("[["), "{map}");
    let board = text(&e, "static/media/drawings/Board.canvas.svg");
    assert!(board.contains("<a href=\"../../public/\"") && board.contains("a card"), "{board}");
    assert!(board.contains(">Hidden</text>") && !board.contains("/hidden"), "a card of a note that is not published is its text: {board}");
    // the drawing is no note and links nothing in the data files (SITE-038)
    assert_eq!(e.notes.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["N.md", "Public.md"]);
    let data = format!("{}{}", text(&e, "data/graph.json"), text(&e, "data/backlinks.json"));
    assert!(!data.to_lowercase().contains("map") && !data.contains("Plugin") && !data.contains("Board"), "{data}");
    assert!(!everything(&e).contains("Hidden words") && !page(&e, "n.md").contains("Hidden"));
}
