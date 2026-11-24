//! The text of a page (SITE-007, SITE-017…SITE-020, SITE-027…SITE-036): a note's Markdown turned
//! into the Markdown Hugo builds. Links are resolved by the rules of the editor (`Vault::resolve`),
//! a link to a note that is not published becomes its shown text, pictures are written with their
//! width, the files a page names are collected for copying, and what would trip Hugo up (a lone
//! `$`, a `{{<` that Hugo reads as a shortcode, a page-break div) is made safe.

use super::anchor::{self, Anchor};
use super::slug::{encode_url, Layout};
use super::svg;
use crate::drawing::{self, ElementLink, Scene};
use crate::markdown::{self, LinkSpan};
use crate::media::outside::{self, Outside};
use crate::media::{media_kind, Kind};
use crate::vault::{FileKind, Shown, Vault};
use ring::digest;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ops::Range;
use std::path::{Path, PathBuf};

/// What a conversion reads: the vault, where the published notes live, and their page titles.
pub struct Env<'a> {
    pub vault: &'a Vault,
    pub layout: &'a Layout,
    pub titles: &'a HashMap<PathBuf, String>,
}

/// What the conversions of all pages collect.
#[derive(Default)]
pub struct State {
    /// the files of the vault that pages embed or link: copied to `static/` (SITE-006)
    pub vault_media: BTreeSet<PathBuf>,
    /// off-vault files that resolve on this device (SITE-036): their path below `static/` and the file
    pub outside_media: BTreeMap<String, PathBuf>,
    /// the drawings that pages embed (SITE-037): the drawing in the vault and its SVG picture
    pub drawings: BTreeMap<PathBuf, (String, (u32, u32))>,
    pub warnings: Vec<String>,
    /// links written as links, and links written as plain text
    pub links: usize,
    pub plain: usize,
    /// (page, note it links) between published notes: the backlinks and the graph (SITE-039, SITE-040)
    pub edges: BTreeSet<(PathBuf, PathBuf)>,
    anchors: HashMap<PathBuf, Vec<Anchor>>,
}

/// Where a vault file is copied to, below `static/`, and its URL (SITE-036 says `stable path`).
pub fn vault_media_path(rel: &Path) -> String {
    format!("media/files/{}", crate::vault::rel_to_string(rel))
}

/// Where the picture of a drawing is written, below `static/` (SITE-037): the drawing's name and `.svg`.
pub fn drawing_path(rel: &Path) -> String {
    format!("media/drawings/{}.svg", crate::vault::rel_to_string(rel))
}

/// Where an off-vault file is copied to: below a folder named by a hash of the link, so the path
/// is the same on every device and never shows where the file is on this one (SITE-036).
fn outside_media_path(target: &str, abs: &Path) -> String {
    let d = digest::digest(&digest::SHA256, target.trim().as_bytes());
    let hash: String = d.as_ref()[..6].iter().map(|b| format!("{b:02x}")).collect();
    format!("media/outside/{hash}/{}", abs.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())
}

/// Whether a file of the vault may be written to the site (SITE-003…SITE-005, SITE-008): not a
/// hidden file or folder, not a conflict copy, not temp or sync debris, not a mail original.
pub fn file_allowed(rel: &Path) -> bool {
    let hidden = rel.components().any(|c| c.as_os_str().to_string_lossy().starts_with('.'));
    let name = rel.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    let debris = [".part", ".tmp", ".eml", ".ics"].iter().any(|e| name.ends_with(e));
    !hidden && !debris && !is_conflict_copy(rel)
}

/// A file named like a conflict copy (SYNC-008, NET-015), of any kind of file.
pub fn is_conflict_copy(rel: &Path) -> bool {
    let name = rel.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let stem = name.rsplit_once('.').map_or(name.as_str(), |(s, _)| s);
    crate::conflict::original_of(&name).is_some() || crate::conflict::original_of(&format!("{stem}.md")).is_some()
}

/// What a link of a note points to on the site.
enum Target {
    /// a published note: its URL
    Page { note: PathBuf, url: String },
    /// a file that is written: its URL, what kind of media it is (none: some other file), its name
    File { url: String, kind: Option<Kind>, name: String },
    /// a drawing that is written as an SVG picture (SITE-037): its URL, its size and its name
    Drawing { url: String, size: (u32, u32) },
    /// nothing that is written: the link becomes its text
    Plain,
    /// an off-vault file that does not resolve here (SITE-036): the name of the placeholder
    Missing { name: String },
}

fn file_name_of(target: &str) -> String {
    match outside::spot(target) {
        Some(outside::Spot::File(p)) => p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        Some(outside::Spot::Library { rest, .. }) => rest.rsplit('/').next().unwrap_or("").to_string(),
        None => target.rsplit('/').next().unwrap_or(target).to_string(),
    }
}

fn resolve_target(env: &Env, st: &mut State, from: &Path, l: &LinkSpan) -> Target {
    match env.vault.resolve_media(&l.target, Some(from)) {
        Shown::Vault(rel) => match env.vault.file(&rel).map(|f| f.kind) {
            // a name that two notes share is not a link the site can be sure of (SITE-017)
            Some(FileKind::Note) if env.vault.ambiguity(&l.target, Some(from)).is_some() => Target::Plain,
            Some(FileKind::Note) => env.layout.notes.get(&rel).map_or(Target::Plain, |p| Target::Page { note: rel, url: p.url.clone() }),
            Some(FileKind::Other) if file_allowed(&rel) => {
                let url = format!("/{}", encode_url(&vault_media_path(&rel)));
                let kind = media_kind(&rel);
                let name = rel.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                st.vault_media.insert(rel);
                Target::File { url, kind, name }
            }
            // an embedded drawing is a picture (SITE-037); a link to one is its text
            Some(FileKind::Drawing) if l.embed && file_allowed(&rel) => drawing_target(env, st, from, &rel),
            _ => Target::Plain,
        },
        Shown::Outside(Outside::Found { abs, kind }) => {
            let path = outside_media_path(&l.target, &abs);
            let name = abs.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let url = format!("/{}", encode_url(&path));
            st.outside_media.insert(path, abs);
            Target::File { url, kind: Some(kind), name }
        }
        Shown::Outside(other) => {
            st.warnings.push(format!("{}: {} is not published: {}", crate::vault::rel_to_string(from), l.target, other.notice().unwrap_or_default()));
            Target::Missing { name: file_name_of(&l.target) }
        }
        _ => Target::Plain,
    }
}

/// The address a link of a drawing gets in its picture (SITE-037, SITE-007): a published note, with
/// its heading when it has one, or a web address; none for a note that is not published, does
/// not exist or is ambiguous, which is then only its text.
fn drawing_href(env: &Env, drawing: &Path, link: &ElementLink) -> Option<String> {
    match link {
        ElementLink::Url(u) => ["http://", "https://", "mailto:"].iter().any(|p| u.to_ascii_lowercase().starts_with(p)).then(|| u.clone()),
        ElementLink::Note { target, heading } => {
            let note = env.vault.resolve(target, Some(drawing)).filter(|_| !target.is_empty() && env.vault.ambiguity(target, Some(drawing)).is_none())?;
            let url = env.layout.notes.get(&note)?.url.clone();
            let id = heading.as_deref().and_then(|h| anchor::id_in(&anchor::anchors(&env.vault.notes.get(&note)?.text), h));
            Some(id.map_or(url.clone(), |id| format!("{url}#{}", encode_url(&id))))
        }
    }
}

/// The picture of the drawing `rel`, embedded by the note `from` (SITE-037): drawn once, written
/// below `static/media/drawings/`. A drawing that cannot be read or is empty is a placeholder and a warning.
fn drawing_target(env: &Env, st: &mut State, from: &Path, rel: &Path) -> Target {
    let name = rel.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if !st.drawings.contains_key(rel) {
        let drawn = Scene::read_file(&env.vault.abs(rel)).and_then(|scene| {
            let size = svg::size(&scene).ok_or("it is empty")?;
            let pictures = drawing::pictures::images(&scene, env.vault, rel);
            let svg = svg::scene_svg(&scene, &pictures, &|link| drawing_href(env, rel, link)).ok_or("it is empty")?;
            Ok((svg, size))
        });
        match drawn {
            Ok(d) => {
                st.drawings.insert(rel.to_path_buf(), d);
            }
            Err(why) => {
                let w = format!("{}: the drawing {} is not published: {why}", crate::vault::rel_to_string(from), name);
                if !st.warnings.contains(&w) {
                    st.warnings.push(w);
                }
                return Target::Missing { name };
            }
        }
    }
    Target::Drawing { url: format!("/{}", encode_url(&drawing_path(rel))), size: st.drawings[rel].1 }
}

/// `s` as literal text in Markdown (and nothing Hugo would take for math or a shortcode).
pub fn escape(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if "\\`*_[]<$~|".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// The alt text and the width of an embed (SITE-034): `![[x.png|Sea|300]]` is "Sea" and 300, `![[x.png|300]]`
/// has no alt text, `![alt](x.png)` has no width.
fn embed_parts(l: &LinkSpan) -> (String, Option<u32>) {
    let alias = l.alias.clone().unwrap_or_default();
    match l.embed_width() {
        Some(w) => (alias.rsplit_once('|').map(|(a, _)| a.trim().to_string()).unwrap_or_default(), Some(w as u32)),
        None => (alias, None),
    }
}

/// The text of a link to a note that is written (SITE-019): its alias, else the title of the note
/// (with the heading it points to).
fn page_text(l: &LinkSpan, title: &str) -> String {
    match (&l.alias, &l.heading) {
        (Some(a), _) => a.clone(),
        (None, Some(h)) if l.target.is_empty() => h.clone(),
        (None, Some(h)) => format!("{title} > {h}"),
        (None, None) => title.to_string(),
    }
}

/// An embed for the theme's render hook (SITE-034, SITE-035, SITE-037): the title tells it how the
/// file is shown (`player=audio|video|pdf`, `drawing=WxH`) and how wide (`width=N`), separated by `;`.
fn embed(alt: &str, url: &str, parts: &[String]) -> String {
    if parts.is_empty() {
        format!("![{}]({url})", escape(alt))
    } else {
        format!("![{}]({url} \"shale:{}\")", escape(alt), parts.join(";"))
    }
}

/// The Markdown that stands for one link.
fn render_link(env: &Env, st: &mut State, from: &Path, l: &LinkSpan) -> String {
    let (alt, width) = if l.embed { embed_parts(l) } else { (String::new(), None) };
    // the label of a `[text](…)` link is Markdown already; an alias of a wiki link is text
    let label = |s: &str| if l.markdown_style { s.to_string() } else { escape(s) };
    match resolve_target(env, st, from, l) {
        Target::Page { note, url } => {
            let anchors = st.anchors.entry(note.clone()).or_insert_with(|| env.vault.notes.get(&note).map(|n| anchor::anchors(&n.text)).unwrap_or_default());
            let id = l.heading.as_deref().and_then(|h| anchor::id_in(anchors, h));
            let dest = id.map_or(url.clone(), |id| format!("{url}#{}", encode_url(&id)));
            let title = env.titles.get(&note).cloned().unwrap_or_default();
            if note != from {
                st.edges.insert((from.to_path_buf(), note));
            }
            st.links += 1;
            let text = if l.embed && !alt.is_empty() { alt } else { page_text(l, &title) };
            format!("[{}]({dest})", label(&text))
        }
        Target::File { url, kind: Some(kind), .. } if l.embed => {
            let player = match kind {
                Kind::Image => None,
                Kind::Audio => Some("audio"),
                Kind::Video => Some("video"),
                Kind::Pdf => Some("pdf"),
            };
            let parts: Vec<String> = player.map(|p| format!("player={p}")).into_iter().chain(width.map(|w| format!("width={w}"))).collect();
            embed(&alt, &url, &parts)
        }
        Target::Drawing { url, size: (w, h) } => {
            let parts: Vec<String> = [format!("drawing={w}x{h}")].into_iter().chain(width.map(|w| format!("width={w}"))).collect();
            embed(&alt, &url, &parts)
        }
        Target::File { url, name, .. } => {
            st.links += 1;
            let text = if l.embed {
                if alt.is_empty() {
                    name
                } else {
                    alt
                }
            } else {
                l.alias.clone().unwrap_or(name)
            };
            format!("[{}]({url})", label(&text))
        }
        Target::Missing { name } => {
            st.plain += 1;
            format!("*(media not available: {})*", escape(&name))
        }
        Target::Plain => {
            st.plain += 1;
            let shown = if l.embed {
                if alt.is_empty() {
                    l.target.clone()
                } else {
                    alt
                }
            } else {
                l.display()
            };
            label(&shown)
        }
    }
}

/// `value` of a property with its links taken out (SITE-010): a link to a published note is its
/// shown text, any other link its alias, or nothing when it has none.
pub fn flatten_links(env: &Env, from: &Path, value: &str) -> String {
    let mut out = value.to_string();
    for l in markdown::find_links(value).into_iter().rev() {
        let published = env.vault.resolve(&l.target, Some(from)).filter(|r| env.layout.notes.contains_key(r));
        let text = match (&published, &l.alias) {
            (_, Some(a)) => a.clone(),
            (Some(r), None) => page_text(&l, env.titles.get(r).map_or("", String::as_str)),
            (None, None) => String::new(),
        };
        out.replace_range(l.range.clone(), &text);
    }
    out
}

fn overlaps(a: &Range<usize>, b: &Range<usize>) -> bool {
    a.start < b.end && b.start < a.end
}

/// `$` outside code, math and links that Hugo would take for the start of math (SITE-028): only
/// what Shale reads as math (EDIT-046) is math, so `costs $5 and $10` gets `\$`.
fn dollar_edits(body: &str, keep: &[Range<usize>], edits: &mut Vec<(Range<usize>, String)>) {
    let fenced = markdown::fenced_code_lines(body);
    for (li, (off, line)) in markdown::lines_with_offsets(body).into_iter().enumerate() {
        if fenced.get(li).copied().unwrap_or(false) || !line.contains('$') {
            continue;
        }
        let code: Vec<Range<usize>> = markdown::inline_code_ranges(line).into_iter().map(|r| off + r.start..off + r.end).collect();
        for (i, _) in line.match_indices('$') {
            let at = off + i;
            let escaped = i > 0 && line.as_bytes()[i - 1] == b'\\';
            if !escaped && !code.iter().chain(keep).any(|r| r.contains(&at)) {
                edits.push((at..at + 1, "\\$".into()));
            }
        }
    }
}

/// Hugo reads `{{< … >}}` and `{{% … %}}` in a page as a shortcode, and fails the build on one it does
/// not know: written as Hugo's own escape, `{{</* … */>}}`, a note shows the text as it wrote it.
fn neutralize_shortcodes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = ["{{<", "{{%"].iter().filter_map(|o| rest.find(o)).min() {
        let open = rest.as_bytes()[at + 2] as char;
        let close = if open == '<' { ">}}" } else { "%}}" };
        out.push_str(&rest[..at]);
        let after = &rest[at + 3..];
        match after.find(close) {
            Some(end) => {
                out.push_str(&format!("{{{{{open}/*{}*/{close}", &after[..end]));
                rest = &after[end + 3..];
            }
            None => {
                // no end: Hugo would fail on it, so it is written as characters
                out.push_str("&#123;&#123;");
                out.push(open);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The Markdown Hugo builds for the text `body` of the note `from` (without its front matter).
pub fn convert(env: &Env, st: &mut State, from: &Path, body: &str) -> String {
    let math = markdown::find_math(body);
    let lines = markdown::lines_with_offsets(body);
    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    let mut keep: Vec<Range<usize>> = math.iter().map(|m| m.range.clone()).collect();
    for l in markdown::find_links(body) {
        if math.iter().any(|m| overlaps(&m.range, &l.range)) {
            continue;
        }
        edits.push((l.range.clone(), render_link(env, st, from, &l)));
        keep.push(l.range);
    }
    // a page break leaves no trace (SITE-031)
    for i in markdown::page_break_lines(body) {
        edits.push((lines[i].0..lines[i].0 + lines[i].1.len(), String::new()));
    }
    // markdown inside an aligned block needs blank lines around its two edges (SITE-032)
    for b in markdown::align_blocks(body) {
        edits.push((lines[b.open].0 + lines[b.open].1.len()..lines[b.open].0 + lines[b.open].1.len(), "\n".into()));
        edits.push((lines[b.close].0..lines[b.close].0, "\n".into()));
    }
    dollar_edits(body, &keep, &mut edits);
    edits.sort_by_key(|e| std::cmp::Reverse((e.0.start, e.0.end)));
    let mut out = body.to_string();
    for (range, text) in edits {
        out.replace_range(range, &text);
    }
    neutralize_shortcodes(&out)
}
