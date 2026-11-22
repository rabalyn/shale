//! Changing the properties of Word's XML in place (DOCX-008, DOCX-009): the text of a part is kept
//! and only the bytes of the elements that change are replaced, at the places roxmltree gives
//! (`Edit`, as `import::docx_look` makes them). A property is put where the schema wants it.

use crate::import::docx_look::{child, Edit};
use roxmltree::Node;

/// The children of `w:pPr` in the order of the schema.
pub const PPR: &[&str] = &[
    "pStyle",
    "keepNext",
    "keepLines",
    "pageBreakBefore",
    "framePr",
    "widowControl",
    "numPr",
    "suppressLineNumbers",
    "pBdr",
    "shd",
    "tabs",
    "suppressAutoHyphens",
    "kinsoku",
    "wordWrap",
    "overflowPunct",
    "topLinePunct",
    "autoSpaceDE",
    "autoSpaceDN",
    "bidi",
    "adjustRightInd",
    "snapToGrid",
    "spacing",
    "ind",
    "contextualSpacing",
    "mirrorIndents",
    "suppressOverlap",
    "jc",
    "textDirection",
    "textAlignment",
    "textboxTightWrap",
    "outlineLvl",
    "divId",
    "cnfStyle",
    "rPr",
    "sectPr",
    "pPrChange",
];

/// The children of `w:rPr` in the order of the schema.
pub const RPR: &[&str] = &[
    "rStyle",
    "rFonts",
    "b",
    "bCs",
    "i",
    "iCs",
    "caps",
    "smallCaps",
    "strike",
    "dstrike",
    "outline",
    "shadow",
    "emboss",
    "imprint",
    "noProof",
    "snapToGrid",
    "vanish",
    "webHidden",
    "color",
    "spacing",
    "w",
    "kern",
    "position",
    "sz",
    "szCs",
    "highlight",
    "u",
    "effect",
    "bdr",
    "shd",
    "fitText",
    "vertAlign",
    "rtl",
    "cs",
    "em",
    "lang",
    "eastAsianLayout",
    "specVanish",
    "oMath",
];

/// The children of `w:sectPr` in the order of the schema.
pub const SECT_PR: &[&str] = &[
    "headerReference",
    "footerReference",
    "footnotePr",
    "endnotePr",
    "type",
    "pgSz",
    "pgMar",
    "paperSrc",
    "pgBorders",
    "lnNumType",
    "pgNumType",
    "cols",
    "formProt",
    "vAlign",
    "noEndnote",
    "titlePg",
    "textDirection",
    "bidi",
    "rtlGutter",
    "docGrid",
    "printerSettings",
    "sectPrChange",
];

/// A change to the children of a properties element.
pub enum Op {
    /// the element `<w:name …/>` as written here takes the place of the one of that name, or is put where it belongs
    Set(&'static str, String),
    /// the attributes of the element of that name are set (the others stay); there is one made when it is not there
    Attrs(&'static str, Vec<(&'static str, String)>),
    /// the element of that name goes
    Remove(&'static str),
}

/// One child: its name and its text.
struct Kid {
    name: String,
    xml: String,
    attrs: Vec<(String, String)>,
}

fn escape(v: &str) -> String {
    v.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;")
}

fn element(name: &str, attrs: &[(String, String)]) -> String {
    let attrs: String = attrs.iter().map(|(k, v)| format!(" w:{k}=\"{}\"", escape(v))).collect();
    format!("<w:{name}{attrs}/>")
}

/// The text of `props` (the children of a properties element, `None`: there are none yet) after `ops`, in the order of `order`
/// where something is put in; what is not known to `order` stays where it is and is passed by new ones.
pub fn changed_children(src: &str, props: Option<Node>, order: &[&str], ops: &[Op]) -> String {
    let mut kids: Vec<Kid> = props
        .map(|p| {
            p.children().filter(|c| c.is_element()).map(|c| Kid { name: c.tag_name().name().to_string(), xml: src[c.range()].to_string(), attrs: c.attributes().map(|a| (a.name().to_string(), a.value().to_string())).collect() }).collect()
        })
        .unwrap_or_default();
    for op in ops {
        match op {
            Op::Remove(name) => kids.retain(|k| k.name != *name),
            Op::Set(name, xml) => put(&mut kids, order, Kid { name: (*name).to_string(), xml: xml.clone(), attrs: vec![] }),
            Op::Attrs(name, set) => {
                let mut attrs = kids.iter().find(|k| k.name == *name).map(|k| k.attrs.clone()).unwrap_or_default();
                for (k, v) in set {
                    match attrs.iter_mut().find(|(have, _)| have == k) {
                        Some(a) => a.1 = v.clone(),
                        None => attrs.push(((*k).to_string(), v.clone())),
                    }
                }
                put(&mut kids, order, Kid { name: (*name).to_string(), xml: element(name, &attrs), attrs });
            }
        }
    }
    kids.iter().map(|k| k.xml.as_str()).collect()
}

fn put(kids: &mut Vec<Kid>, order: &[&str], new: Kid) {
    let rank = |name: &str| order.iter().position(|n| *n == name).unwrap_or(order.len());
    if let Some(at) = kids.iter().position(|k| k.name == new.name) {
        let name = new.name.clone();
        kids[at] = new;
        // one element of a name only
        let mut seen = false;
        kids.retain(|k| k.name != name || !std::mem::replace(&mut seen, true));
        return;
    }
    let at = kids.iter().position(|k| rank(&k.name) > rank(&new.name)).unwrap_or(kids.len());
    kids.insert(at, new);
}

/// The qualified name of an element as written (`w:pPrDefault`).
fn tag<'a>(src: &'a str, n: Node) -> &'a str {
    let r = n.range();
    let inner = &src[r.start + 1..r.end];
    &inner[..inner.find(|c: char| c.is_whitespace() || c == '/' || c == '>').unwrap_or(inner.len())]
}

/// The edit that applies `ops` to the properties element `kind` (`pPr` or `rPr`) of `holder`, a style
/// or a part of the document defaults. The element is made when it is not there: before the
/// elements that follow it in a style, else last. `None` when nothing changes (a holder
/// without any element).
pub fn edit_holder(src: &str, holder: Node, kind: &'static str, order: &[&str], ops: &[Op]) -> Option<Edit> {
    let existing = child(holder, kind);
    let inner = changed_children(src, existing, order, ops);
    let new = if inner.is_empty() { String::new() } else { format!("<w:{kind}>{inner}</w:{kind}>") };
    if let Some(e) = existing {
        return Some(Edit { start: e.range().start, end: e.range().end, with: new });
    }
    if new.is_empty() {
        return None;
    }
    let later: &[&str] = if kind == "pPr" { &["rPr", "tblPr", "trPr", "tcPr", "tblStylePr"] } else { &["tblPr", "trPr", "tcPr", "tblStylePr"] };
    let at = holder.children().filter(|c| c.is_element()).find(|c| later.contains(&c.tag_name().name())).map(|c| c.range().start);
    match (at, holder.children().rfind(|c| c.is_element())) {
        (Some(at), _) => Some(Edit { start: at, end: at, with: new }),
        (None, Some(last)) => Some(Edit { start: last.range().end, end: last.range().end, with: new }),
        // `<w:pPrDefault/>`: the element has no inside to put it in
        (None, None) => {
            let r = holder.range();
            let name = tag(src, holder);
            Some(Edit { start: r.start, end: r.end, with: format!("<{name}>{new}</{name}>") })
        }
    }
}
