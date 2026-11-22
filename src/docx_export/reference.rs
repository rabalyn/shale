//! The reference document pandoc takes its styles, page and numbering from (DOCX-007, DOCX-008): a
//! copy of the original Word file with pandoc's own styles merged in, or pandoc's default with the
//! note's `print-*` properties laid into its styles. Only `word/styles.xml` changes, and only at the
//! elements that do (`xml`).

use super::look::PrintLook;
use super::package::Package;
use super::xml::{self, Op};
use crate::import::docx_look::{apply, child, is_w, val, Edit};
use crate::pdf::tex::proc::{self, Ending};
use crate::pdf::write::{FontKind, Rgba};
use roxmltree::{Document, Node};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

const STYLES: &str = "word/styles.xml";

/// pandoc's default reference document (`pandoc --print-default-data-file reference.docx`), made in `dir`.
pub fn default_reference(pandoc: &Path, dir: &Path, cancel: Option<&AtomicBool>) -> Result<Vec<u8>, String> {
    let path = dir.join("default-reference.docx");
    let setup = |cmd: &mut std::process::Command| {
        cmd.args(["--print-default-data-file", "reference.docx"]);
    };
    let go = |cmd| {
        let out = crate::fsutil::open_private_log(&path, false)?;
        proc::run_to_file(cmd, out, Duration::from_secs(20), cancel)
    };
    let run = crate::pandoc_limit::run(pandoc, setup, go).map_err(|e| format!("pandoc could not be started: {e}"))?;
    match run.ending {
        Ending::Exited(Some(0)) => crate::fsutil::read(&path).map_err(|e| e.to_string()),
        Ending::Cancelled => Err("stopped".into()),
        Ending::TimedOut => Err("pandoc did not give its default reference document in time".into()),
        Ending::Exited(_) if crate::pandoc_limit::overflowed(&run) => Err(crate::pandoc_limit::overflow_message(None)),
        Ending::Exited(_) => Err(format!("pandoc has no default reference document: {}", run.tail.iter().find(|l| !l.trim().is_empty()).map_or("it stopped without a message", |l| l.trim()))),
    }
}

fn name_of(style: Node) -> Option<String> {
    child(style, "name").and_then(|n| val(n, "val")).map(str::to_lowercase)
}

/// The styles of the original (`original`, the text of its `word/styles.xml`) and, after them, every style of
/// pandoc's default (`default`) that the original lacks (DOCX-007): one with the id or the name (case ignored)
/// of a style of the original is not added. *Body Text* comes without its paragraph spacing, a `w:default` is
/// dropped (the original has its defaults), and a `w:basedOn`, `w:next` or `w:link` naming a style of pandoc's
/// that the original has under another id names that id.
pub fn merge_styles(original: &str, default: &str) -> Result<String, String> {
    let o = crate::xml_limits::parse(original).map_err(|e| format!("its styles cannot be read: {e}"))?;
    let d = crate::xml_limits::parse(default).map_err(|e| format!("pandoc's default styles cannot be read: {e}"))?;
    let styles = |doc: &'_ Document<'_>| doc.root_element().children().filter(|n| is_w(*n, "style")).filter_map(|n| Some((val(n, "styleId")?.to_string(), name_of(n)?, n.id()))).collect::<Vec<_>>();
    let have = styles(&o);
    let have_ids: HashSet<String> = have.iter().map(|(id, ..)| id.to_lowercase()).collect();
    let have_names: HashMap<&str, &str> = have.iter().map(|(id, name, _)| (name.as_str(), id.as_str())).collect();
    let all = styles(&d);
    let default_names: HashMap<&str, &str> = all.iter().map(|(id, name, _)| (id.as_str(), name.as_str())).collect();
    let mut add = String::new();
    for (id, name, node) in &all {
        if have_ids.contains(&id.to_lowercase()) || have_names.contains_key(name.as_str()) {
            continue;
        }
        let style = d.get_node(*node).expect("a node of this document");
        let range = style.range();
        let mut edits: Vec<Edit> = Vec::new();
        let head_end = style.first_element_child().map_or(range.end, |c| c.range().start);
        if let Some(at) = default[range.start..head_end].find(" w:default=\"1\"") {
            edits.push(Edit { start: range.start + at, end: range.start + at + " w:default=\"1\"".len(), with: String::new() });
        }
        if id == "BodyText" {
            edits.extend(xml::edit_holder(default, style, "pPr", xml::PPR, &[Op::Remove("spacing")]));
        }
        for link in style.children().filter(|c| ["basedOn", "next", "link"].iter().any(|n| is_w(*c, n))) {
            let renamed = val(link, "val").and_then(|v| default_names.get(v)).and_then(|n| have_names.get(n)).filter(|new| Some(**new) != val(link, "val"));
            if let Some(new) = renamed {
                edits.push(Edit { start: link.range().start, end: link.range().end, with: format!("<w:{} w:val=\"{new}\"/>", link.tag_name().name()) });
            }
        }
        // the edits are of the whole text: shift them to the style's own
        let own: Vec<Edit> = edits.into_iter().map(|e| Edit { start: e.start - range.start, end: e.end - range.start, with: e.with }).collect();
        let text = &default[range.clone()];
        add.push_str(&apply(text, own).unwrap_or_else(|| text.to_string()));
        add.push('\n');
    }
    let close = original.rfind("</").ok_or("its styles have no end")?;
    Ok(apply(original, vec![Edit { start: close, end: close, with: add }]).expect("one edit"))
}

/// The reference document made of the original Word file (`original`: its bytes), DOCX-007: its parts, its
/// styles with pandoc's merged in (`default`: pandoc's default reference document).
pub fn from_original(original: &[u8], default: &[u8]) -> Result<Vec<u8>, String> {
    let mut file = Package::read(original).map_err(|e| format!("the original Word file cannot be the template: {e}"))?;
    let theirs = Package::read(default)?;
    let merged = merge_styles(file.text(STYLES).map_err(|e| format!("the original Word file cannot be the template: {e}"))?, theirs.text(STYLES)?).map_err(|e| format!("the original Word file cannot be the template: {e}"))?;
    file.set(STYLES, merged);
    file.pack()
}

/// The reference document of a note without an original (DOCX-008): pandoc's default, its styles changed by what `look` says.
/// A note that says nothing gets the default as it is.
pub fn from_props(default: &[u8], look: &PrintLook) -> Result<Vec<u8>, String> {
    if !look.styled() {
        return Ok(default.to_vec());
    }
    let mut file = Package::read(default)?;
    let styles = lay_props(file.text(STYLES)?, look)?;
    file.set(STYLES, styles);
    file.pack()
}

/// What changes in which part of the styles.
#[derive(PartialEq, Eq, Clone)]
enum Holder {
    /// `w:pPrDefault`
    ParagraphDefaults,
    /// `w:rPrDefault`
    RunDefaults,
    Style(String),
}

fn font_name(f: FontKind) -> &'static str {
    match f {
        FontKind::Serif => "Times New Roman",
        FontKind::Mono => "Courier New",
        _ => "Calibri",
    }
}

fn hex(c: Rgba) -> String {
    format!("{:02X}{:02X}{:02X}", c.0, c.1, c.2)
}

/// Half points as a whole number, as Word's `w:sz` wants them.
fn half_points(pt: f32) -> String {
    ((pt * 2.0).round() as u32).to_string()
}

fn headings(upto: usize) -> impl Iterator<Item = String> {
    (1..=upto).map(|n| format!("Heading{n}"))
}

/// The styles with `look` laid into them (DOCX-008): the defaults and the styles Word's own names for headings
/// have. A style that is not there is not changed.
pub fn lay_props(styles: &str, look: &PrintLook) -> Result<String, String> {
    let doc = crate::xml_limits::parse(styles).map_err(|e| format!("pandoc's default styles cannot be read: {e}"))?;
    let root = doc.root_element();
    // (where, which properties element) → what is done to it
    let mut plan: Vec<((Holder, &'static str), Vec<Op>)> = Vec::new();
    let mut add = |holder: Holder, kind: &'static str, op: Op| match plan.iter_mut().find(|(k, _)| k.0 == holder && k.1 == kind) {
        Some((_, ops)) => ops.push(op),
        None => plan.push(((holder, kind), vec![op])),
    };
    let style = |id: String| Holder::Style(id);
    if let Some(f) = look.font {
        let name = font_name(f);
        let fonts = || Op::Set("rFonts", format!("<w:rFonts w:ascii=\"{name}\" w:hAnsi=\"{name}\" w:eastAsia=\"{name}\" w:cs=\"{name}\"/>"));
        add(Holder::RunDefaults, "rPr", fonts());
        // every style that names a theme font
        for s in root.children().filter(|n| is_w(*n, "style")) {
            let themed = child(s, "rPr").and_then(|r| child(r, "rFonts")).is_some_and(|f| f.attributes().any(|a| a.name().to_lowercase().ends_with("theme")));
            if let (true, Some(id)) = (themed, val(s, "styleId")) {
                add(style(id.to_string()), "rPr", fonts());
            }
        }
    }
    if let Some(pt) = look.font_pt {
        add(Holder::RunDefaults, "rPr", Op::Set("sz", format!("<w:sz w:val=\"{}\"/>", half_points(pt))));
        add(Holder::RunDefaults, "rPr", Op::Set("szCs", format!("<w:szCs w:val=\"{}\"/>", half_points(pt))));
    }
    if let Some(f) = look.line_spacing {
        add(Holder::ParagraphDefaults, "pPr", Op::Attrs("spacing", vec![("line", ((f * 240.0).round() as u32).to_string()), ("lineRule", "auto".into())]));
    }
    if let Some(pt) = look.paragraph_spacing {
        add(Holder::ParagraphDefaults, "pPr", Op::Attrs("spacing", vec![("after", ((pt * 20.0).round() as u32).to_string())]));
        add(style("BodyText".into()), "pPr", Op::Remove("spacing"));
    }
    if look.justify {
        add(style("Normal".into()), "pPr", Op::Set("jc", "<w:jc w:val=\"both\"/>".into()));
        for id in headings(9) {
            add(style(id), "pPr", Op::Set("jc", "<w:jc w:val=\"left\"/>".into()));
        }
    }
    if let Some(c) = look.heading_color {
        for id in headings(9).chain(["Title".to_string()]) {
            add(style(id), "rPr", Op::Set("color", format!("<w:color w:val=\"{}\"/>", hex(c))));
        }
    }
    if let Some(bold) = look.heading_bold {
        let (b, cs) = if bold { ("<w:b/>", "<w:bCs/>") } else { ("<w:b w:val=\"0\"/>", "<w:bCs w:val=\"0\"/>") };
        for id in headings(9) {
            add(style(id.clone()), "rPr", Op::Set("b", b.into()));
            add(style(id), "rPr", Op::Set("bCs", cs.into()));
        }
    }
    if let Some(sizes) = look.heading_sizes {
        for (id, pt) in headings(6).zip(sizes) {
            add(style(id.clone()), "rPr", Op::Set("sz", format!("<w:sz w:val=\"{}\"/>", half_points(pt))));
            add(style(id), "rPr", Op::Set("szCs", format!("<w:szCs w:val=\"{}\"/>", half_points(pt))));
        }
    }
    if let Some(level) = look.break_before {
        for id in headings(level as usize) {
            add(style(id), "pPr", Op::Set("pageBreakBefore", "<w:pageBreakBefore/>".into()));
        }
    }
    let mut edits = Vec::new();
    for ((holder, kind), ops) in &plan {
        let node = match holder {
            Holder::ParagraphDefaults => child(root, "docDefaults").and_then(|d| child(d, "pPrDefault")),
            Holder::RunDefaults => child(root, "docDefaults").and_then(|d| child(d, "rPrDefault")),
            Holder::Style(id) => root.children().find(|n| is_w(*n, "style") && val(*n, "styleId") == Some(id.as_str())),
        };
        let order = if *kind == "pPr" { xml::PPR } else { xml::RPR };
        edits.extend(node.and_then(|n| xml::edit_holder(styles, n, kind, order, ops)));
    }
    Ok(apply(styles, edits).unwrap_or_else(|| styles.to_string()))
}

/// The width of the text of the page in inches (DOCX-005): the page and the margins of the note when it has them, else
/// those of the first section of the reference document, which is the page pandoc takes (DOCX-009); a page that says
/// nothing is Word's 8.5 in with margins of 1 in.
pub fn text_width(reference: &[u8], look: &PrintLook) -> f32 {
    const TWIPS_PER_INCH: f32 = 1440.0;
    let section = Package::read(reference).ok().and_then(|p| {
        let document = p.text("word/document.xml").ok()?;
        let doc = crate::xml_limits::parse(document).ok()?;
        let sect = doc.descendants().find(|n| is_w(*n, "sectPr"))?;
        let get = |part: &str, attr: &str| child(sect, part).and_then(|n| val(n, attr)).and_then(|v| v.parse::<f32>().ok());
        Some((get("pgSz", "w"), get("pgMar", "left"), get("pgMar", "right")))
    });
    let (page, left, right) = section.unwrap_or((None, None, None));
    let mm = |v: f32| v / 25.4;
    let width = look.paper.map(|[w, _]| mm(w)).or(page.map(|w| w / TWIPS_PER_INCH)).unwrap_or(8.5);
    let margins = look.margins.map(|[_, r, _, l]| mm(l + r)).or(left.zip(right).map(|(l, r)| (l + r) / TWIPS_PER_INCH)).unwrap_or(2.0);
    (width - margins).max(1.0)
}
