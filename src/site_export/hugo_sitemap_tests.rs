//! A real build of the site map page and of `sitemap.xml` (SITE-080, SITE-081, SITE-082): the
//! tree of the published notes, the files that come only with an address of the site, and the
//! links of a site that is served below a path. Hugo is an external program (BUILD-018): a test
//! skips with a printed note when it is missing.

use super::hugo_tests::{attrs, build, hugo_here};
use super::test_support::{has, text, Fixture};

fn notes(tag: &str, rules: &str) -> Fixture {
    Fixture::with(
        tag,
        rules,
        &[
            ("Alpha.md", "#pub\nTo [[Roses]].\n"),
            ("Sitemap.md", "#pub\nA note called Sitemap.\n"),
            ("Garden/Garden.md", "---\ntitle: The garden\n---\n#pub\nThe folder note.\n"),
            ("Garden/Roses.md", "#pub\nRed.\n"),
            ("Garden/Deep/Moss.md", "#pub\nGreen.\n"),
            ("Plain/Tulip.md", "#pub\nA folder without a folder note.\n"),
            ("Hidden.md", "Hidden words, see [[Alpha]].\n"),
            ("Secret/Vault.md", "Hidden folder.\n"),
        ],
    )
}

// spec: SITE-080
#[test]
fn site_080_the_site_map_page_lists_the_published_notes_as_a_tree_and_clashes_with_no_note() {
    if !hugo_here() {
        return;
    }
    let f = notes("h080", "\"include_tags\": [\"pub\"]");
    let (public, said) = build(&f);
    assert!(!said.contains("WARN"), "{said}");
    let map = std::fs::read_to_string(public.join("sitemap.html")).expect("the site map is at /sitemap.html");
    // a note called Sitemap keeps its own page
    assert!(super::hugo_tests::html(&public, "sitemap").contains("A note called Sitemap."));
    // the tree: folders with their notes below them, the folder note as the folder's link, titles of SITE-021
    let tree: Vec<&str> = map.split("<ul class=\"page-list sitetree\">").nth(1).unwrap().split("</ul></li></ul>").next().unwrap().lines().collect();
    let flat = tree.join("\n");
    let at = |needle: &str| flat.find(needle).unwrap_or_else(|| panic!("{needle} in {flat}"));
    assert!(at("href=\"/garden/\">The garden</a>") < at("href=\"/garden/deep/\">Deep</a>"), "{flat}");
    assert!(at("href=\"/garden/deep/\">Deep</a>") < at("href=\"/garden/deep/moss/\">Moss</a>"), "{flat}");
    assert!(at("href=\"/garden/deep/moss/\">Moss</a>") < at("href=\"/garden/roses/\">Roses</a>"), "folders first, then the notes of the folder: {flat}");
    assert!(at("href=\"/plain/\">Plain</a>") < at("href=\"/plain/tulip/\">Tulip</a>"), "a folder without a folder note is still a link: {flat}");
    for link in ["/alpha/\">Alpha", "/sitemap/\">Sitemap"] {
        assert!(flat.contains(link), "{link}: {flat}");
    }
    // only the published notes
    assert!(!map.contains("idden") && !map.contains("Secret") && !map.contains("Vault.md"), "{map}");
    // every link of the page leads to a page of the site; no script and nothing from another site
    let built = |href: &str| public.join(href.trim_start_matches('/')).join("index.html").is_file();
    let hrefs: Vec<String> = attrs(&map, "href").into_iter().filter(|h| h.ends_with('/') && h != "/").collect();
    assert!(hrefs.len() >= 7, "{hrefs:?}");
    for h in &hrefs {
        assert!(built(h), "{h} is a page");
    }
    let main = map.split("<main>").nth(1).unwrap();
    assert!(!main.contains("<script") && !map.contains("src=\"http") && !map.contains("href=\"http"), "{map}");
    assert!(!map.contains("data-pagefind-body"), "the site map is not indexed");
    // every page links to it next to the graph, and it works without an address of the site
    assert!(super::hugo_tests::html(&public, "alpha").contains("href=\"/sitemap.html\">Site map</a>"));
    assert!(map.contains("href=\"/graph.html\">Graph</a>") && map.contains("href=\"/sitemap.html\">Site map</a>"));
    assert!(!public.join("sitemap.xml").exists() && !public.join("robots.txt").exists(), "no address, no sitemap.xml and no robots.txt");
}

// spec: SITE-081, SITE-082, SITE-010
#[test]
fn site_081_with_an_address_the_site_has_a_sitemap_xml_of_the_published_pages_and_a_robots_txt() {
    if !hugo_here() {
        return;
    }
    let f = notes("h081", "\"include_tags\": [\"pub\"], \"site_address\": \"https://wiki.example.org\"");
    let (public, _) = build(&f);
    let xml = std::fs::read_to_string(public.join("sitemap.xml")).expect("sitemap.xml is written");
    let mut locs: Vec<&str> = xml.split("<loc>").skip(1).map(|p| p.split("</loc>").next().unwrap()).collect();
    locs.sort();
    assert_eq!(
        locs,
        [
            "https://wiki.example.org/",
            "https://wiki.example.org/alpha/",
            "https://wiki.example.org/garden/",
            "https://wiki.example.org/garden/deep/",
            "https://wiki.example.org/garden/deep/moss/",
            "https://wiki.example.org/garden/roses/",
            "https://wiki.example.org/plain/",
            "https://wiki.example.org/plain/tulip/",
            "https://wiki.example.org/sitemap/",
        ],
        "{xml}"
    );
    // nothing that the pages do not show: no date, no tag page, no unpublished note
    for not in ["lastmod", "changefreq", "priority", "/tags/", "idden", "ecret", "graph", "sitemap.html"] {
        assert!(!xml.contains(not), "{not}: {xml}");
    }
    let robots = std::fs::read_to_string(public.join("robots.txt")).expect("robots.txt is written");
    assert_eq!(robots.trim(), "User-agent: *\nAllow: /\nSitemap: https://wiki.example.org/sitemap.xml");
    // the page links by path, as without an address
    assert!(super::hugo_tests::html(&public, "alpha").contains("href=\"/sitemap.html\">Site map</a>"));
    assert!(std::fs::read_to_string(public.join("sitemap.html")).unwrap().contains("href=\"/garden/roses/\">Roses</a>"));
}

// spec: SITE-082
#[test]
fn site_082_a_site_served_below_a_path_links_below_it() {
    if !hugo_here() {
        return;
    }
    let f = notes("h082", "\"include_tags\": [\"pub\"], \"site_address\": \"https://host.example/wiki\"");
    let (public, _) = build(&f);
    let alpha = super::hugo_tests::html(&public, "alpha");
    for link in ["href=\"/wiki/graph.html\">Graph</a>", "href=\"/wiki/sitemap.html\">Site map</a>", "href=\"/wiki/\">"] {
        assert!(alpha.contains(link), "{link}: {alpha}");
    }
    // a link of the text, the stylesheet and the link to a tag follow the path as well
    assert!(alpha.contains("href=\"/wiki/shale-site.css\"") && alpha.contains("<a href=\"/wiki/garden/roses/\">Roses</a>") && alpha.contains("href=\"/wiki/tags/pub/\""), "{alpha}");
    assert!(!alpha.contains("href=\"/garden/") && !alpha.contains("href=\"/\""), "{alpha}");
    assert!(super::hugo_tests::html(&public, "garden/roses").contains("href=\"/wiki/alpha/\">Alpha</a>"), "the backlinks follow the path");
    let map = std::fs::read_to_string(public.join("sitemap.html")).unwrap();
    assert!(map.contains("href=\"/wiki/garden/roses/\">Roses</a>") && map.contains("href=\"/wiki/alpha/\">Alpha</a>"), "{map}");
    assert!(!map.contains("href=\"/alpha/\""), "no link leaves the path: {map}");
    let graph = std::fs::read_to_string(public.join("graph.html")).unwrap();
    assert!(graph.contains("data-base=\"/wiki/\"") && graph.contains("src=\"/wiki/shale-graph.js\""), "{graph}");
    let xml = std::fs::read_to_string(public.join("sitemap.xml")).unwrap();
    assert!(xml.contains("<loc>https://host.example/wiki/garden/roses/</loc>") && xml.contains("<loc>https://host.example/wiki/</loc>"), "{xml}");
    assert!(std::fs::read_to_string(public.join("robots.txt")).unwrap().contains("Sitemap: https://host.example/wiki/sitemap.xml"));
}

// spec: SITE-082, SITE-081
#[test]
fn site_082_the_config_has_the_address_and_the_sitemap_only_when_the_vault_has_an_address() {
    let toml = |rules: &str| -> toml::Table {
        let f = Fixture::with("s082", rules, &[("N.md", "x\n")]);
        let e = f.export();
        assert!(has(&e, "themes/shale/layouts/sitemap.xml") && has(&e, "themes/shale/layouts/robots.txt") && has(&e, "themes/shale/layouts/index.sitetree.html"));
        text(&e, "hugo.toml").parse().unwrap()
    };
    let kinds = |t: &toml::Table| t["disableKinds"].as_array().unwrap().iter().map(|k| k.as_str().unwrap().to_string()).collect::<Vec<_>>();
    let none = toml("\"all_notes\": true");
    assert_eq!(none["baseURL"].as_str(), Some("/"));
    assert_eq!(kinds(&none), ["RSS", "sitemap", "robotsTXT"]);
    let some = toml("\"all_notes\": true, \"site_address\": \" https://wiki.example.org/a \"");
    assert_eq!(some["baseURL"].as_str(), Some("https://wiki.example.org/a/"), "trimmed, with the slash");
    assert_eq!(kinds(&some), ["RSS"]);
    assert_eq!(some["enableRobotsTXT"].as_bool(), Some(true));
    assert!(!none.contains_key("enableRobotsTXT"));
    // the site map page is an output of the home page in both
    for t in [&none, &some] {
        assert_eq!(t["outputs"]["home"].as_array().unwrap().len(), 3);
        assert_eq!(t["outputFormats"]["SiteTree"]["baseName"].as_str(), Some("sitemap"));
    }
    // a site without a note has the same config
    let empty = Fixture::with("s082e", "\"site_address\": \"https://wiki.example.org\"", &[]);
    let t: toml::Table = text(&empty.export(), "hugo.toml").parse().unwrap();
    assert_eq!(t["baseURL"].as_str(), Some("https://wiki.example.org/"));
}

// spec: SITE-081
#[test]
fn site_081_an_address_that_is_not_allowed_stops_the_export_and_says_where_it_is_set() {
    let f = Fixture::with("s081", "\"all_notes\": true, \"site_address\": \"https://me:secret@wiki.example.org/\"", &[("N.md", "x\n")]);
    let vault = crate::vault::Vault::open(&f.dir).unwrap();
    let e = super::plan::build(&vault).err().expect("refused");
    assert!(e.contains("user name or password") && e.contains("publish.site_address") && !e.contains("secret"), "{e}");
}
