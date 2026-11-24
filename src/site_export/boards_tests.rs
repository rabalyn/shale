//! The snapshot of a published board (SITE-045): lanes, cards, order, and nothing of a card whose
//! note is not published (SITE-007, SITE-010).

use super::hugo_tests::{attrs, build, html, hugo_here};
use super::test_support::{everything, page, text, Fixture};
use serde_json::Value;

const BOARD: &str = "---\ntype: board\n---\n# Sprint\n\nOur sprint, in words.\n\n\
    ## Open\nquery: #todo\nsort: due asc\n\n\
    ## Doing\nquery: status:doing\n\n\
    ## Pinned\n- [[Secret pin]]\n- [[Pinned pub]]\n\n\
    ## Only hidden\nquery: #hushtag\n";

fn files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("Sprint.md", "#pub\n\n"),
        ("Late task.md", "---\ndue: 2026-03-01\nstatus: open\n---\n#pub #todo\nLate words.\n"),
        ("Early task.md", "---\ndue: 2026-01-01\nstatus: open\n---\n#pub #todo\nEarly words.\n"),
        ("Active task.md", "---\nstatus: doing\n---\n#pub\nActive words.\n"),
        ("Pinned pub.md", "#pub\nPinned words.\n"),
        ("Zebrafish plan.md", "---\ndue: 2026-02-01\nstatus: doing\n---\n#todo #hushtag\nZebrafish secret words.\n"),
        ("Secret pin.md", "Quokka secret pin words.\n"),
    ]
}

fn fixture(tag: &str) -> Fixture {
    let f = Fixture::with(tag, "\"include_tags\": [\"pub\"]", &files());
    // the board is chosen by its tag; its lanes are lines below the tag line
    f.file("Sprint.md", &format!("---\ntype: board\ntags: [pub]\n---\n{}", BOARD.split("---\n").last().unwrap()));
    f
}

fn lanes(e: &super::plan::Export) -> Vec<Value> {
    let data: Value = serde_json::from_str(&text(e, "data/boards.json")).unwrap();
    data["sprint.md"]["lanes"].as_array().unwrap().clone()
}

fn titles(lane: &Value) -> Vec<&str> {
    lane["cards"].as_array().unwrap().iter().map(|c| c["title"].as_str().unwrap()).collect()
}

// spec: SITE-045
#[test]
fn site_045_a_board_page_has_its_lanes_and_cards_in_the_boards_order_with_the_chips() {
    let f = fixture("b045");
    let e = f.export();
    let lanes = lanes(&e);
    let names: Vec<&str> = lanes.iter().map(|l| l["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Open", "Doing", "Pinned", "Only hidden"]);
    assert_eq!(titles(&lanes[0]), ["Early task", "Late task"], "sorted by the lane's sort");
    assert_eq!(titles(&lanes[1]), ["Active task"]);
    assert_eq!(titles(&lanes[2]), ["Pinned pub"]);
    let early = &lanes[0]["cards"][0];
    assert_eq!(early["url"], "/early-task/");
    let chips: Vec<&str> = early["chips"].as_array().unwrap().iter().map(|c| c.as_str().unwrap()).collect();
    assert!(chips.contains(&"2026-01-01") && chips.contains(&"open") && chips.contains(&"#todo"), "{chips:?}");
    // the board's own words are the text of the page, its lane lines are not
    let p = page(&e, "sprint.md");
    assert!(p.contains("Our sprint, in words."), "{p}");
    assert!(!p.contains("query:") && !p.contains("Secret pin") && !p.contains("hushtag"), "{p}");
}

// spec: SITE-045, SITE-007, SITE-010
#[test]
fn site_045_a_card_of_a_note_that_is_not_published_is_left_out_without_a_trace() {
    let f = fixture("b045p");
    let e = f.export();
    let lanes = lanes(&e);
    assert!(titles(&lanes[3]).is_empty(), "a lane of only hidden cards is empty: it does not count them");
    assert!(lanes[3].get("unresolved").is_none());
    let all = everything(&e);
    for secret in ["Zebrafish", "zebrafish", "hushtag", "Quokka", "Secret pin", "secret-pin", "secret words"] {
        assert!(!all.contains(secret), "{secret} is in the output");
    }
}

// spec: SITE-045, SITE-067
#[test]
fn site_045_a_lane_that_cannot_be_resolved_shows_a_placeholder_and_a_warning() {
    let f = Fixture::with("b045w", "\"all_notes\": true", &[("Broken.md", "---\ntype: board\n---\nWords.\n\n## Bad\nquery: status:(\n\n## Fine\n- [[Card]]\n"), ("Card.md", "Card.\n")]);
    let e = f.export();
    let data: Value = serde_json::from_str(&text(&e, "data/boards.json")).unwrap();
    let lanes = data["broken.md"]["lanes"].as_array().unwrap();
    assert_eq!(lanes[0]["unresolved"], true);
    assert!(lanes[0]["cards"].as_array().unwrap().is_empty());
    assert_eq!(titles(&lanes[1]), ["Card"]);
    assert!(e.warnings.iter().any(|w| w.contains("Broken.md") && w.contains("lane “Bad”")), "{:?}", e.warnings);
    assert!(!everything(&e).contains("status:("), "the query is not written");
}

// spec: SITE-045
#[test]
fn site_045_a_site_without_a_board_has_an_empty_board_data_file() {
    let f = Fixture::with("b045n", "\"all_notes\": true", &[("A.md", "Plain.\n")]);
    let e = f.export();
    assert_eq!(text(&e, "data/boards.json").trim(), "{}");
}

// spec: SITE-045, SITE-046
#[test]
fn site_045_hugo_builds_the_board_page_read_only_with_links_to_published_cards() {
    if !hugo_here() {
        return;
    }
    let f = fixture("h045");
    let (public, _) = build(&f);
    let page = html(&public, "sprint");
    let board = page.split("<div class=\"board\">").nth(1).and_then(|b| b.split("</section>\n</div>").next().map(|_| b)).unwrap_or_else(|| panic!("{page}"));
    for lane in ["Open", "Doing", "Pinned", "Only hidden"] {
        assert!(board.contains(&format!("<h2>{lane} <span class=\"count\">")), "{lane}: {page}");
    }
    assert!(board.find("Early task").unwrap() < board.find("Late task").unwrap(), "the order of the lane");
    let links = attrs(board, "href");
    assert!(links.contains(&"/early-task/".to_string()) && links.contains(&"/pinned-pub/".to_string()), "{links:?}");
    assert!(page.contains("Our sprint, in words."));
    let made = board.split("</div>\n</section>").next().unwrap();
    assert!(!made.contains("<script") && !made.contains("<form") && !made.contains("<button") && !made.contains("draggable"), "read-only: {made}");
    assert!(!page.contains("Zebrafish") && !page.contains("Quokka"));
    let css = std::fs::read_to_string(public.join("shale-site.css")).unwrap();
    assert!(css.contains(".board { display: flex; gap: 12px; overflow-x: auto;"), "the lanes scroll sideways");
    // a link to the card's page works: the page of a card is built
    assert!(public.join("early-task/index.html").is_file());
}
