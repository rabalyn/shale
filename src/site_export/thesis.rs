//! Citations, numbering and the bibliography in the site (SITE-077, ADR-0039, #648). The text of a
//! note is rewritten before [`super::body::convert`] reads it: citations and references become
//! links, labelled figures, tables and equations are wrapped with their number and caption, and the
//! bibliography and the lists of figures and tables are written where they stand. It is plain
//! Markdown and HTML (the config of the site allows both), so no script runs in the browser. What
//! each says comes from [`crate::thesis::View`], the same the reading view and the PDF take.

use crate::markdown::citations::Kind;
use crate::thesis::{Marker, Shown, View};

/// `s` as HTML text: what a raw HTML block of a page holds (`$` too, which Hugo would read as math).
fn html(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '$' => out.push_str("&#36;"),
            c => out.push(c),
        }
    }
    out
}

/// The `id` of a label in the page: `fig-results`, `eq-energy`.
fn label_id(kind: Kind, id: &str) -> String {
    format!("{}-{}", kind.prefix(), id.replace([':', '.'], "-"))
}

/// The `id` of an entry of the bibliography.
fn cite_id(key: &str) -> String {
    format!("cite-{}", key.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '-' }).collect::<String>())
}

/// The text of the note `text` as the site reads it: the thesis syntax written as Markdown and HTML.
pub fn apply(view: &View, text: &str) -> String {
    let marked = with_links(view, text);
    wrapped(view, &marked)
}

/// Step one: citations and references are links, labels are gone. No line is added or removed.
fn with_links(view: &View, text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for span in view.spans(text) {
        if span.range.start < at {
            continue;
        }
        out.push_str(&text[at..span.range.start]);
        let link = |href: String, shown: &str| format!("<a class=\"thesis-ref\" href=\"#{href}\">{}</a>", super::body::escape(&html(shown)));
        match &span.shown {
            Shown::Label => {}
            Shown::Citation { text, keys, .. } => out.push_str(&keys.first().map_or_else(|| super::body::escape(&html(text)), |k| link(cite_id(k), text))),
            Shown::Reference { text, targets, .. } => out.push_str(&targets.first().map_or_else(|| super::body::escape(&html(text)), |(k, id)| link(label_id(*k, id), text))),
            Shown::Broken { .. } => out.push_str(&text[span.range.clone()]),
        }
        at = span.range.end;
    }
    out.push_str(&text[at..]);
    out
}

/// A group of lines of the text that is written another way.
struct Replace {
    /// the lines taken: first and last, both included
    first: usize,
    last: usize,
    text: String,
}

/// Step two: what takes whole lines, by line (step one left the lines as they were).
fn wrapped(view: &View, text: &str) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut edits: Vec<Replace> = Vec::new();
    let cap = |e: &crate::markdown::labels::Entry| format!("<figcaption>{}</figcaption>", html(&view.caption_text(e)));
    let wrap = |class: &str, tag: &str, id: String, before: &str, body: &str, after: &str| {
        let mut parts = vec![format!("<{tag} id=\"{id}\" class=\"{class}\">")];
        parts.extend([before, body, after].into_iter().filter(|s| !s.is_empty()).map(str::to_string));
        parts.push(format!("</{tag}>"));
        format!("\n{}\n", parts.join("\n\n"))
    };
    // figures: an embed on its line with its label
    for e in view.floats(Kind::Figure) {
        let (before, after) = if view.caption_above(Kind::Figure) { (cap(e), String::new()) } else { (String::new(), cap(e)) };
        edits.push(Replace { first: e.line, last: e.line, text: wrap("shale-figure", "figure", label_id(Kind::Figure, &e.id), &before, lines[e.line].trim(), &after) });
    }
    // tables: the pipe table and the caption line under it
    for b in crate::table::find_tables(text) {
        if let Some(e) = view.label_on(Kind::Table, b.lines.end) {
            let (before, after) = if view.caption_above(Kind::Table) { (cap(e), String::new()) } else { (String::new(), cap(e)) };
            let table = lines[b.lines.start..b.lines.end].join("\n");
            edits.push(Replace { first: b.lines.start, last: b.lines.end, text: wrap("shale-table", "figure", label_id(Kind::Table, &e.id), &before, &table, &after) });
        }
    }
    // equations: the number at the right
    for m in crate::markdown::find_math(text).into_iter().filter(|m| m.block) {
        if let Some(e) = view.labels.entries.iter().find(|e| e.kind == Kind::Equation && e.source == view.index && m.lines.contains(&e.line)) {
            let number = e.number.as_ref().map_or(String::new(), |n| format!("<span class=\"eq-number\">({})</span>", html(n)));
            let body = lines[m.lines.clone()].join("\n");
            edits.push(Replace { first: m.lines.start, last: m.lines.end - 1, text: wrap("shale-equation", "div", label_id(Kind::Equation, &e.id), "", &body, &number) });
        }
    }
    // sections: an anchor above the heading
    for e in view.labels.entries.iter().filter(|e| e.kind == Kind::Section && e.source == view.index) {
        edits.push(Replace { first: e.line, last: e.line, text: format!("\n<a id=\"{}\"></a>\n\n{}", label_id(Kind::Section, &e.id), lines[e.line]) });
    }
    // the markers: the lists where they stand, and none for a list with nothing in it
    let blocks = view.blocks(text);
    let fenced = crate::markdown::fenced_code_lines(text);
    let front = crate::markdown::front_matter_lines(text);
    for (n, line) in lines.iter().enumerate().skip(front) {
        if let Some(marker) = Marker::of(line.trim()).filter(|_| !fenced.get(n).copied().unwrap_or(false)) {
            let shown = if blocks.iter().any(|b| b.line == n) { list(view, marker) } else { String::new() };
            edits.push(Replace { first: n, last: n, text: shown });
        }
    }
    edits.sort_by_key(|e| std::cmp::Reverse(e.first));
    let mut out: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
    for e in edits {
        out.splice(e.first..=e.last, [e.text]);
    }
    let mut out = out.join("\n");
    // the bibliography at the end of the note, when no marker put it elsewhere
    if blocks.iter().any(|b| b.line >= lines.len() && b.what == Marker::Bibliography) {
        out.push_str(&format!("\n\n{}\n", list(view, Marker::Bibliography)));
    }
    out
}

/// A list of the document as Markdown and HTML: a heading and the entries.
fn list(view: &View, what: Marker) -> String {
    match what {
        Marker::Bibliography => {
            let entries: Vec<String> = view
                .references()
                .iter()
                .map(|r| format!("<p class=\"bib-entry\" id=\"{}\">{}{}</p>", cite_id(&r.key), r.label.as_ref().map_or(String::new(), |l| format!("<span class=\"bib-label\">{}</span> ", html(l))), html(&r.text)))
                .collect();
            format!("\n## {}\n\n<div class=\"shale-bibliography\">\n{}\n</div>\n", view.lang.bibliography_heading(), entries.join("\n"))
        }
        Marker::ListOfFigures | Marker::ListOfTables => {
            let kind = if what == Marker::ListOfFigures { Kind::Figure } else { Kind::Table };
            let items: Vec<String> = view.floats(kind).into_iter().map(|e| format!("<li><a href=\"#{}\">{}</a></li>", label_id(kind, &e.id), html(&view.caption_text(e)))).collect();
            format!("\n## {}\n\n<ul class=\"shale-list\">\n{}\n</ul>\n", view.lang.list_heading(kind), items.join("\n"))
        }
    }
}
