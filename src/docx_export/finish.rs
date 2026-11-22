//! Finishing the file pandoc made (DOCX-009): what its docx writer cannot say is said here, at the
//! elements that change (`xml`), and the parts are packed again, compressed.

use super::look::PrintLook;
use super::package::Package;
use super::xml::{self, Op};
use crate::import::docx_look::{apply, child, is_w, val, Edit};
use roxmltree::{Document, Node};
use std::collections::HashSet;

const DOCUMENT: &str = "word/document.xml";
const STYLES: &str = "word/styles.xml";

/// The name of the bookmark the filter puts in a paragraph that is aligned (DOCX-006), then the `w:jc`.
const ALIGN_MARK: &str = "_shale_jc_";

/// The namespace of `r:` (`r:embed`, `r:id`, `r:link`): the attributes that name a relationship.
const RELATIONSHIPS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// Twips (a twentieth of a point) of millimetres.
fn twips(mm: f32) -> String {
    ((mm / 25.4 * 1440.0).round() as u32).to_string()
}

/// `docx`, finished: the alignment, the colour styles, the page of the note and no pictures that nothing uses.
pub fn finish(docx: &[u8], look: &PrintLook) -> Result<Vec<u8>, String> {
    let mut file = Package::read(docx).map_err(|e| format!("pandoc's file cannot be finished: {e}"))?;
    let broken = |what: &str, e: roxmltree::Error| format!("pandoc's file cannot be finished: {what} cannot be read: {e}");
    let document = file.text(DOCUMENT)?.to_string();
    let styles = file.text(STYLES)?.to_string();
    let doc = Document::parse(&document).map_err(|e| broken(DOCUMENT, e))?;
    let mut edits = alignment(&document, &doc);
    edits.extend(page(&document, &doc, look));
    let sty = Document::parse(&styles).map_err(|e| broken(STYLES, e))?;
    let new_styles = apply(&styles, colour_styles(&styles, &sty));
    if let Some(text) = new_styles {
        file.set(STYLES, text);
    }
    if let Some(text) = apply(&document, edits) {
        file.set(DOCUMENT, text);
    }
    prune_media(&mut file)?;
    file.pack()
}

/// The `w:jc` of the paragraphs that have the filter's mark, and the marks gone.
fn alignment(src: &str, doc: &Document) -> Vec<Edit> {
    let mut paragraphs: Vec<(Node, Vec<Node>)> = Vec::new();
    for mark in doc.descendants().filter(|n| is_w(*n, "bookmarkStart") && val(*n, "name").is_some_and(|v| v.starts_with(ALIGN_MARK))) {
        let Some(p) = mark.ancestors().find(|a| is_w(*a, "p")) else { continue };
        match paragraphs.iter_mut().find(|(q, _)| q.id() == p.id()) {
            Some((_, marks)) => marks.push(mark),
            None => paragraphs.push((p, vec![mark])),
        }
    }
    let mut edits = Vec::new();
    for (p, marks) in paragraphs {
        for mark in &marks {
            edits.push(Edit { start: mark.range().start, end: mark.range().end, with: String::new() });
            // a mark is a bookmark that ends where it starts
            let end = mark.next_siblings().skip(1).find(|n| n.is_element()).filter(|n| is_w(*n, "bookmarkEnd") && val(*n, "id") == val(*mark, "id"));
            edits.extend(end.map(|e| Edit { start: e.range().start, end: e.range().end, with: String::new() }));
        }
        let jc = marks.last().and_then(|m| val(*m, "name")).map(|n| &n[ALIGN_MARK.len()..]).filter(|v| ["center", "right", "both", "left"].contains(v));
        let Some(jc) = jc else { continue };
        let set = [Op::Set("jc", format!("<w:jc w:val=\"{jc}\"/>"))];
        match child(p, "pPr") {
            Some(_) => edits.extend(xml::edit_holder(src, p, "pPr", xml::PPR, &set)),
            None => {
                // a paragraph's properties come first
                let at = p.first_child().map_or(p.range().end, |c| c.range().start);
                edits.push(Edit { start: at, end: at, with: format!("<w:pPr>{}</w:pPr>", xml::changed_children(src, None, xml::PPR, &set)) });
            }
        }
    }
    edits
}

/// The page of the note in the last section of the body: `w:pgSz` and `w:pgMar`.
fn page(src: &str, doc: &Document, look: &PrintLook) -> Option<Edit> {
    let sect = doc.root_element().children().find(|n| is_w(*n, "body"))?.children().rfind(|n| is_w(*n, "sectPr"))?;
    let mut ops: Vec<Op> = Vec::new();
    if let Some([w, h]) = look.paper {
        let orient = if w > h { " w:orient=\"landscape\"" } else { "" };
        ops.push(Op::Set("pgSz", format!("<w:pgSz w:w=\"{}\" w:h=\"{}\"{orient}/>", twips(w), twips(h))));
    }
    if let Some([top, right, bottom, left]) = look.margins {
        let mut attrs: Vec<(&'static str, String)> = vec![("top", twips(top)), ("right", twips(right)), ("bottom", twips(bottom)), ("left", twips(left))];
        // a page has these distances to its header, its footer and its binding
        if child(sect, "pgMar").is_none() {
            attrs.extend([("header", "708".to_string()), ("footer", "708".to_string()), ("gutter", "0".to_string())]);
        }
        ops.push(Op::Attrs("pgMar", attrs));
    }
    let (first, last) = (sect.first_element_child()?, sect.last_element_child()?);
    if ops.is_empty() {
        return None;
    }
    Some(Edit { start: first.range().start, end: last.range().end, with: xml::changed_children(src, Some(sect), xml::SECT_PR, &ops) })
}

/// The colour and the background a style of the filter's names stands for (DOCX-004): `Shale colour RRGGBB`,
/// `Shale colour RRGGBB on RRGGBB` and `Shale on RRGGBB`.
fn colours(name: &str) -> Option<(Option<&str>, Option<&str>)> {
    let rest = name.strip_prefix("Shale ")?;
    let (fg, bg) = match rest.strip_prefix("colour ") {
        Some(r) => match r.split_once(" on ") {
            Some((fg, bg)) => (Some(fg), Some(bg)),
            None => (Some(r), None),
        },
        None => (None, Some(rest.strip_prefix("on ")?)),
    };
    let rgb = |c: &str| c.len() == 6 && c.chars().all(|ch| ch.is_ascii_hexdigit());
    (fg.is_none_or(rgb) && bg.is_none_or(rgb)).then_some((fg, bg))
}

/// The `w:color` and `w:shd` of the character styles that stand for colours.
fn colour_styles(src: &str, doc: &Document) -> Vec<Edit> {
    let mut edits = Vec::new();
    for style in doc.root_element().children().filter(|n| is_w(*n, "style") && val(*n, "type") == Some("character")) {
        let Some((fg, bg)) = child(style, "name").and_then(|n| val(n, "val")).and_then(colours) else { continue };
        let mut ops = Vec::new();
        if let Some(c) = fg {
            ops.push(Op::Set("color", format!("<w:color w:val=\"{}\"/>", c.to_uppercase())));
        }
        if let Some(c) = bg {
            ops.push(Op::Set("shd", format!("<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"{}\"/>", c.to_uppercase())));
        }
        edits.extend(xml::edit_holder(src, style, "rPr", xml::RPR, &ops));
    }
    edits
}

/// A relationship's target as the name of the part in the zip: relative to the folder of the part the relationships are of.
fn target_name(rels: &str, target: &str) -> String {
    // `word/_rels/document.xml.rels` names parts relative to `word/`
    let mut parts: Vec<&str> = if target.starts_with('/') { vec![] } else { rels.rsplit_once("/_rels/").map_or(vec![], |(dir, _)| dir.split('/').collect()) };
    for p in target.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    parts.join("/")
}

/// Leave out the pictures in `word/media/` that the part they belong to does not use (DOCX-009): pandoc copies those
/// of the reference document with their relationships. A relationship to a picture whose id no `r:` attribute of the part
/// holds goes with it; a picture that no relationship is left to name goes too.
fn prune_media(file: &mut Package) -> Result<(), String> {
    let all: Vec<String> = file.names().map(String::from).collect();
    let mut named: HashSet<String> = HashSet::new();
    for rels in all.iter().filter(|n| n.ends_with(".rels")) {
        let text = file.text(rels)?.to_string();
        let broken = |e: roxmltree::Error| format!("pandoc's file cannot be finished: {rels} cannot be read: {e}");
        let doc = Document::parse(&text).map_err(broken)?;
        // the part the relationships belong to, `word/document.xml` for `word/_rels/document.xml.rels`
        let partner = rels.rsplit_once("/_rels/").map(|(dir, name)| format!("{dir}/{}", name.trim_end_matches(".rels"))).filter(|p| all.contains(p));
        let used: Option<HashSet<String>> = match partner {
            Some(p) => {
                let part = file.text(&p)?;
                let d = Document::parse(part).map_err(|e| format!("pandoc's file cannot be finished: {p} cannot be read: {e}"))?;
                Some(d.descendants().flat_map(|n| n.attributes().filter(|a| a.namespace() == Some(RELATIONSHIPS)).map(|a| a.value().to_string()).collect::<Vec<_>>()).collect())
            }
            None => None,
        };
        let mut edits = Vec::new();
        for r in doc.descendants().filter(|n| n.is_element() && n.tag_name().name() == "Relationship" && n.attribute("TargetMode") != Some("External")) {
            let Some(target) = r.attribute("Target").map(|t| target_name(rels, t)) else { continue };
            let unused = used.as_ref().is_some_and(|u| !r.attribute("Id").is_some_and(|id| u.contains(id)));
            if target.starts_with("word/media/") && unused {
                edits.push(Edit { start: r.range().start, end: r.range().end, with: String::new() });
            } else {
                named.insert(target);
            }
        }
        if let Some(text) = apply(&text, edits) {
            file.set(rels, text);
        }
    }
    file.retain(|name| !name.starts_with("word/media/") || named.contains(name));
    Ok(())
}
