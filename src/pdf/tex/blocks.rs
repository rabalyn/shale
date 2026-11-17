//! A note's body as blocks (TEX-008): headings, paragraphs, quotes, lists, fenced code, tables,
//! embeds on their own line and rules. Line by line, by the rules the native export and the
//! reading view use (`pdf::export::export_note`): a line of text is a line of the paragraph, a
//! heading needs a space after its hashes, `---` is a rule, an embed is alone on its line.

use crate::markdown;
use crate::pdf::export::list_marker;

/// What a list item is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    Bullet,
    /// The number it is written with.
    Number(u64),
    /// A task and whether it is done.
    Task(bool),
}

/// One line of a list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListLine {
    /// How far the line is indented (a tab is four).
    pub indent: usize,
    pub item: Item,
    pub text: String,
}

/// One line of a quote and how deep it is quoted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuoteLine {
    pub depth: usize,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    /// A heading: its level as written (1 to 6), its text, and the line it is on.
    Heading {
        level: usize,
        text: String,
        line: usize,
    },
    /// The lines of a paragraph.
    Para(Vec<String>),
    Quote(Vec<QuoteLine>),
    List(Vec<ListLine>),
    /// A fenced block: the language word and the lines.
    Code {
        lang: String,
        lines: Vec<String>,
    },
    Table(crate::table::Block),
    /// A formula on lines of its own that Shale can typeset: its TeX (TEX-028). One it cannot
    /// typeset is a [`Block::Code`] of its lines.
    Math(String),
    /// An embed alone on its line.
    Embed(markdown::LinkSpan),
    Rule,
}

/// How far a line is indented, a tab counting four.
fn indent_of(line: &str) -> usize {
    line.chars().take_while(|c| c.is_whitespace()).map(|c| if c == '\t' { 4 } else { 1 }).sum()
}

/// The lines of the note below its front matter as blocks. `plain_tables` is the built-in
/// export's switch and not used here: tables are always tables.
pub fn parse(text: &str) -> Vec<Block> {
    let lines = markdown::lines_with_offsets(text);
    let fenced = markdown::fenced_code_lines(text);
    let tables = crate::table::find_tables(text);
    let maths = markdown::find_math(text);
    let notes = markdown::find_footnotes(text);
    let mut out: Vec<Block> = Vec::new();
    let mut i = markdown::front_matter_lines(text);
    // lines of a paragraph, of a list and of a quote that are not yet closed
    let mut para: Vec<String> = Vec::new();
    let mut list: Vec<ListLine> = Vec::new();
    let mut quote: Vec<QuoteLine> = Vec::new();
    macro_rules! close {
        () => {
            if !para.is_empty() {
                out.push(Block::Para(std::mem::take(&mut para)));
            }
            if !list.is_empty() {
                out.push(Block::List(std::mem::take(&mut list)));
            }
            if !quote.is_empty() {
                out.push(Block::Quote(std::mem::take(&mut quote)));
            }
        };
    }
    while i < lines.len() {
        // a formula on lines of its own (a `$$` block, a `math` fence): math when Shale can typeset it, else its text as code
        if let Some(m) = maths.iter().find(|m| m.block && m.lines.start == i) {
            close!();
            let tex = text[m.tex.clone()].trim();
            out.push(if crate::math::typeset(tex, 14.0, true).is_ok() { Block::Math(tex.to_string()) } else { Block::Code { lang: String::new(), lines: lines[m.lines.clone()].iter().map(|l| l.1.to_string()).collect() } });
            i = m.lines.end.max(i + 1);
            continue;
        }
        // a footnote definition is written where its reference is (TEX-028)
        if let Some(d) = notes.def_at(i) {
            close!();
            i = d.lines.end;
            continue;
        }
        if let Some(b) = tables.iter().find(|b| b.lines.start == i) {
            close!();
            out.push(Block::Table(b.clone()));
            i = b.lines.end;
            continue;
        }
        let line = lines[i].1;
        let trimmed = line.trim();
        if fenced.get(i).copied().unwrap_or(false) {
            close!();
            let lang = crate::syntax::fence_language(line);
            let mut code = Vec::new();
            i += 1;
            while i < lines.len() && !(lines[i].1.trim_start().starts_with("```") || lines[i].1.trim_start().starts_with("~~~")) {
                code.push(lines[i].1.to_string());
                i += 1;
            }
            i += 1;
            out.push(Block::Code { lang, lines: code });
            continue;
        }
        let links = markdown::find_links(line);
        if links.len() == 1 && links[0].embed && trimmed.len() == links[0].range.len() {
            close!();
            out.push(Block::Embed(links[0].clone()));
        } else if trimmed.is_empty() {
            close!();
        } else if trimmed == "---" || trimmed == "***" || trimmed == "___" {
            close!();
            out.push(Block::Rule);
        } else if let Some(level) = markdown::heading_level(line) {
            close!();
            out.push(Block::Heading { level, text: line[level..].trim().to_string(), line: i });
        } else if let Some((_, done, rest, _)) = markdown::parse_task_line(line) {
            if list.is_empty() {
                close!();
            }
            list.push(ListLine { indent: indent_of(line), item: Item::Task(done), text: rest.trim().to_string() });
        } else if let Some((marker, rest)) = list_marker(line) {
            if list.is_empty() {
                close!();
            }
            let item = marker.trim_end_matches('.').parse::<u64>().map_or(Item::Bullet, Item::Number);
            list.push(ListLine { indent: indent_of(line), item, text: line[rest..].trim().to_string() });
        } else if trimmed.starts_with('>') {
            if quote.is_empty() {
                close!();
            }
            // `> a`, `>> b`, `> > b`: the depth is the number of marks in front of the text
            let mut rest = trimmed;
            let mut depth = 0;
            while let Some(r) = rest.strip_prefix('>') {
                depth += 1;
                rest = r.trim_start();
            }
            quote.push(QuoteLine { depth, text: rest.to_string() });
        } else {
            if para.is_empty() {
                close!();
            }
            para.push(trimmed.to_string());
        }
        i += 1;
    }
    close!();
    out
}

/// The footnotes a note defines: label → the text of the definition, its first line after the label
/// and the lines that go on with it joined by spaces (TEX-028). The first definition of a label counts.
pub fn footnote_texts(text: &str) -> std::collections::HashMap<String, String> {
    let lines = markdown::lines_with_offsets(text);
    let mut out = std::collections::HashMap::new();
    for d in markdown::find_footnotes(text).defs {
        let first = lines[d.lines.start].1;
        let parts = std::iter::once(first[d.marker.end..].trim()).chain(lines[d.lines.start + 1..d.lines.end].iter().map(|l| l.1.trim()));
        out.entry(d.label).or_insert_with(|| parts.filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" "));
    }
    out
}
