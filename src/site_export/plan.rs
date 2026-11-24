//! Everything an export writes (SITE-001, SITE-002, SITE-009): worked out in memory from the vault,
//! so a dry run lists it and a real run writes it without having to decide anything more.

use super::boards;
use super::body::{self, Env, State};
use super::front::{self, Page};
use super::rules::{self, Candidate};
use super::slug::Layout;
use super::theme;
use crate::markdown;
use crate::vault::{rel_to_string, sort_key, Vault};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// What a file of the site is made of.
pub enum Content {
    Text(String),
    /// a file of this computer, copied
    Copy(PathBuf),
}

/// A file of the site tree, by its path below the output folder (`/` separated).
pub struct OutFile {
    pub path: String,
    pub content: Content,
}

/// The site an export writes.
pub struct Export {
    pub files: Vec<OutFile>,
    /// the notes that are published: (path in the vault, the file below `content/`), in path order
    pub notes: Vec<(String, String)>,
    /// the media files that are written (their paths in the tree)
    pub media: Vec<String>,
    /// links written as links, and links written as plain text (SITE-015)
    pub links: usize,
    pub plain: usize,
    pub warnings: Vec<String>,
    /// why the site has no note, when it has none (SITE-001, SITE-051)
    pub empty_because: Option<String>,
}

fn secs(t: Option<std::time::SystemTime>) -> Option<i64> {
    t.and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64)
}

/// The text of a note below its front matter.
fn body_of(text: &str) -> &str {
    let n = markdown::front_matter_lines(text);
    let at: usize = text.split_inclusive('\n').take(n).map(str::len).sum();
    &text[at..]
}

fn flagged(text: &str) -> bool {
    markdown::front_matter_values(text).iter().any(|(k, v)| k == "folder-note" && v.first().is_some_and(|v| v.trim().eq_ignore_ascii_case("true")))
}

/// The name of the vault: its folder's name.
pub fn vault_name(vault: &Vault) -> String {
    vault.root.file_name().map(|n| n.to_string_lossy().into_owned()).filter(|n| !n.is_empty()).unwrap_or_else(|| "Vault".to_string())
}

/// The text of the hello-world page of a site without a note (SITE-001): fixed, nothing of the
/// vault, its notes or this computer is in it (SITE-010).
const HELLO_TITLE: &str = "Hello from Shale";

/// The user guide note about publishing, on the project's web page (the program has no other base
/// address of the guide); a test checks that the file exists.
pub const HELLO_GUIDE: &str = "https://github.com/rabalyn/shale/blob/main/docs/userguide/Publishing%20a%20vault.md";

fn hello_page(exported: i64) -> String {
    let (days, sod) = (exported.div_euclid(86_400), exported.rem_euclid(86_400));
    let (y, m, d) = crate::calendar::time::civil_from_days(days);
    let page = Page { title: HELLO_TITLE, tags: &[], lastmod: None };
    format!(
        "{}This page was published with Shale. It shows that publishing and the build of the site work.\n\nNo notes are chosen for publishing yet.\n\nTo publish notes, open Settings → Publishing in Shale for your vault, turn publishing on, choose notes (all notes, folders or tags, or `publish: true` in a note) and publish again. [How publishing works]({HELLO_GUIDE})\n\nExported at {y:04}-{m:02}-{d:02} {} UTC.\n",
        front::front_matter(&page, "", &|v| v.to_string()),
        crate::calendar::time::hhmm(sod)
    )
}

/// The site without a note: the config, the theme, empty data files and the hello-world page. The
/// site is called after Shale, not after the vault, so that nothing of the vault is in it.
fn empty_site(why: String, address: Option<&str>) -> Export {
    let mut files = theme::files("Shale site", address).into_iter().map(|(path, text)| OutFile { path, content: Content::Text(text) }).collect::<Vec<_>>();
    for name in ["backlinks", "graph"] {
        let text = if name == "graph" { "{\"nodes\": [], \"links\": []}\n" } else { "{}\n" };
        files.push(OutFile { path: format!("data/{name}.json"), content: Content::Text(text.into()) });
    }
    files.push(OutFile { path: "content/_index.md".into(), content: Content::Text(hello_page(crate::calendar::time::now())) });
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Export { files, notes: vec![], media: vec![], links: 0, plain: 0, warnings: vec![], empty_because: Some(why) }
}

/// The notes of `vault` the publishing rules choose, in path order (SITE-002).
pub fn chosen_notes(vault: &Vault, settings: &crate::vault_state::VaultSettings) -> Vec<PathBuf> {
    let publish = &settings.publish;
    let mut notes: Vec<&PathBuf> = vault.notes.keys().collect();
    notes.sort_by_cached_key(|p| sort_key(p));
    notes
        .into_iter()
        .filter(|rel| {
            let n = &vault.notes[*rel];
            // a note that cannot be read is no note (DATA-008); a file that may not be written is no page (SITE-003…SITE-005)
            n.unreadable.is_none() && body::file_allowed(rel) && vault.file(rel).is_some_and(|f| f.kind == crate::vault::FileKind::Note) && rules::chosen(publish, &settings.people_dir, &Candidate { rel, text: &n.text, tags: &n.tags })
        })
        .cloned()
        .collect()
}

/// Work out the site of `vault`, whose libraries (MEDIA-014) are set. Reads the vault, never writes it.
pub fn build(vault: &Vault) -> Result<Export, String> {
    let settings = rules::read_settings(&vault.root)?;
    let address = super::address::check(&settings.publish.site_address).map_err(|e| format!("{e} (publish.site_address in .shale/settings.json)"))?;
    if !settings.publish.enabled {
        return Ok(empty_site("Publishing is off for this vault (publish.enabled in .shale/settings.json), so the site has no note".into(), address.as_deref()));
    }
    let published = chosen_notes(vault, &settings);
    if published.is_empty() {
        return Ok(empty_site("No note is chosen for publishing by the rules of this vault, so the site has no note".into(), address.as_deref()));
    }
    let text_of = |rel: &Path| vault.notes.get(rel).map_or("", |n| n.text.as_str());
    let name = vault_name(vault);
    let layout = Layout::build(&published, &name, &|n| flagged(text_of(n)));
    let folder_title = |dir: &Path| if dir.as_os_str().is_empty() { name.clone() } else { dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default() };
    // the title of a page: a folder note is its folder's page
    let section_of: HashMap<&PathBuf, &PathBuf> = layout.folder_notes.iter().map(|(dir, note)| (note, dir)).collect();
    let titles: HashMap<PathBuf, String> = published
        .iter()
        .map(|rel| {
            let own = markdown::front_matter_values(text_of(rel)).iter().any(|(k, v)| k == "title" && v.iter().any(|t| !t.trim().is_empty()));
            let title = match section_of.get(rel) {
                Some(dir) if !own => folder_title(dir),
                _ => front::page_title(rel, text_of(rel)),
            };
            // a title with a link in it could name a note that is not published: the title line is the title then
            let title = if title.contains("[[") || title.contains("](") { crate::vault::title_of(&rel.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()).to_string() } else { title };
            (rel.clone(), title)
        })
        .collect();
    let env = Env { vault, layout: &layout, titles: &titles };
    let mut st = State::default();
    let mut files: Vec<OutFile> = Vec::new();
    let mut notes = Vec::new();
    let mut board_data: BTreeMap<String, Value> = BTreeMap::new();
    for rel in &published {
        let text = text_of(rel);
        let place = &layout.notes[rel];
        let lastmod = crate::note_props::modified(text, secs(vault.file(rel).and_then(|f| f.modified))).map(crate::note_props::format_timestamp);
        // a board's page has its own words as the text and its lanes as the snapshot (SITE-045); the
        // tags in its lanes' queries are not its tags (they could be those of unpublished notes)
        let is_board = boards::is_board(text);
        let intro = if is_board { boards::intro(text) } else { String::new() };
        let tags = if is_board { boards::tags(text, &intro) } else { vault.notes[rel].tags.clone() };
        let page = Page { title: &titles[rel], tags: &tags, lastmod };
        let head = front::front_matter(&page, text, &|v| body::flatten_links(&env, rel, v));
        let converted = body::convert(&env, &mut st, rel, if is_board { &intro } else { body_of(text) });
        if is_board {
            let (snap, warnings) = boards::snapshot(&env, rel);
            board_data.insert(place.content.clone(), snap);
            st.warnings.extend(warnings);
        }
        files.push(OutFile { path: format!("content/{}", place.content), content: Content::Text(format!("{head}{converted}")) });
        notes.push((rel_to_string(rel), place.content.clone()));
    }
    // a folder without a folder note is a section with no text (SITE-041)
    for (dir, place) in &layout.sections {
        if !layout.folder_notes.contains_key(dir) {
            let page = Page { title: &folder_title(dir), tags: &[], lastmod: None };
            files.push(OutFile { path: format!("content/{}", place.content), content: Content::Text(front::front_matter(&page, "", &|v| v.to_string())) });
        }
    }
    files.extend(data_files(&layout, &titles, &st.edges));
    files.push(OutFile { path: "data/boards.json".into(), content: Content::Text(format!("{}\n", serde_json::to_string_pretty(&json!(board_data)).unwrap_or_default())) });
    let mut media = Vec::new();
    for rel in &st.vault_media {
        let path = format!("static/{}", body::vault_media_path(rel));
        let abs = vault.abs(rel);
        if is_lfs_pointer(&abs) {
            st.warnings.push(format!("{} is a Git LFS pointer that is not downloaded: it is written as it is", rel_to_string(rel)));
        }
        files.push(OutFile { path: path.clone(), content: Content::Copy(abs) });
        media.push(path);
    }
    // the pictures of the drawings that pages embed (SITE-037)
    for (rel, (svg, _)) in &st.drawings {
        let path = format!("static/{}", body::drawing_path(rel));
        files.push(OutFile { path: path.clone(), content: Content::Text(svg.clone()) });
        media.push(path);
    }
    for (path, abs) in &st.outside_media {
        let path = format!("static/{path}");
        files.push(OutFile { path: path.clone(), content: Content::Copy(abs.clone()) });
        media.push(path);
    }
    files.extend(theme::files(&name, address.as_deref()).into_iter().map(|(path, text)| OutFile { path, content: Content::Text(text) }));
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(Export { files, notes, media, links: st.links, plain: st.plain, warnings: st.warnings, empty_because: None })
}

fn is_lfs_pointer(abs: &Path) -> bool {
    use std::io::Read;
    let mut head = [0u8; 40];
    let n = crate::fsutil::open_read(abs).and_then(|mut f| f.read(&mut head)).unwrap_or(0);
    head[..n].starts_with(b"version https://git-lfs")
}

/// `data/backlinks.json` and `data/graph.json` (SITE-039, SITE-040): the published notes only.
fn data_files(layout: &Layout, titles: &HashMap<PathBuf, String>, edges: &BTreeSet<(PathBuf, PathBuf)>) -> Vec<OutFile> {
    let mut backlinks: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    let mut graph_links: BTreeSet<(String, String)> = BTreeSet::new();
    for (from, to) in edges {
        let (Some(a), Some(b)) = (layout.notes.get(from), layout.notes.get(to)) else { continue };
        if a.url == b.url {
            continue;
        }
        let entry = json!({"title": titles[from], "url": a.url});
        let list = backlinks.entry(b.content.clone()).or_default();
        if !list.contains(&entry) {
            list.push(entry);
        }
        graph_links.insert((a.url.clone(), b.url.clone()));
    }
    for list in backlinks.values_mut() {
        list.sort_by_key(|v| (v["title"].as_str().unwrap_or("").to_lowercase(), v["url"].as_str().unwrap_or("").to_string()));
    }
    let mut nodes: Vec<(String, String)> = layout.notes.iter().filter_map(|(rel, p)| titles.get(rel).map(|t| (p.url.clone(), t.clone()))).collect();
    nodes.sort();
    nodes.dedup();
    let graph = json!({
        "nodes": nodes.iter().map(|(url, title)| json!({"id": url, "title": title, "url": url})).collect::<Vec<_>>(),
        "links": graph_links.iter().map(|(a, b)| json!({"source": a, "target": b})).collect::<Vec<_>>(),
    });
    let pretty = |v: &Value| format!("{}\n", serde_json::to_string_pretty(v).unwrap_or_default());
    vec![OutFile { path: "data/backlinks.json".into(), content: Content::Text(pretty(&json!(backlinks))) }, OutFile { path: "data/graph.json".into(), content: Content::Text(pretty(&graph)) }]
}
