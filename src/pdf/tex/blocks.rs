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
    /// A formula on lines of its own that Shale can typeset: its TeX (TEX-029). One it cannot
    /// typeset is a [`Block::Code`] of its lines.
    Math(String),
    /// A formula of that kind with the label `{#eq:id}` (TEX-046): its TeX and the id.
    Equation {
        tex: String,
        id: String,
    },
    /// An embed alone on its line.
    Embed(markdown::LinkSpan),
    /// An embed alone on its line with the label `{#fig:id}` after it (TEX-045): a numbered figure.
    Figure {
        link: markdown::LinkSpan,
        id: String,
    },
    /// A pipe table with the caption line `: Caption {#tbl:id}` below it (TEX-045): a numbered table.
    CaptionedTable {
        table: crate::table::Block,
        caption: String,
        id: String,
    },
    /// A marker line `<!-- shale: bibliography -->`, `list-of-figures` or `list-of-tables` (TEX-047).
    Marker(Marker),
    Rule,
    /// A page-break marker line (TEX-037).
    PageBreak,
    /// The opening and the closing line of an alignment wrapper (TEX-039): what is between them
    /// is the blocks in between.
    AlignStart(markdown::Align),
    AlignEnd(markdown::Align),
}

/// What a marker line `<!-- shale: … -->` asks for (ADR-0039 §5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Marker {
    Bibliography,
    ListOfFigures,
    ListOfTables,
}

impl Marker {
    /// The marker a trimmed line is: `<!-- shale: bibliography -->`, `list-of-figures`, `list-of-tables`.
    pub fn of_line(trimmed: &str) -> Option<Marker> {
        let inner = trimmed.strip_prefix("<!--")?.strip_suffix("-->")?;
        match inner.split_whitespace().collect::<Vec<_>>().as_slice() {
            ["shale:", "bibliography"] => Some(Marker::Bibliography),
            ["shale:", "list-of-figures"] => Some(Marker::ListOfFigures),
            ["shale:", "list-of-tables"] => Some(Marker::ListOfTables),
            _ => None,
        }
    }
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
    let page_breaks = markdown::page_break_lines(text);
    let wrappers = markdown::align_blocks(text);
    // the labels of equations (`{#eq:x}` after a display formula, EDIT-122)
    let equation_labels: Vec<(std::ops::Range<usize>, String)> = if text.contains("{#eq:") {
        markdown::citations::scan(text, None).into_iter().filter_map(|t| if let markdown::citations::Token::Label(l) = t { (l.kind == markdown::citations::Kind::Equation).then_some((l.range, l.id)) } else { None }).collect()
    } else {
        Vec::new()
    };
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
        // the lines of an alignment wrapper are blocks of their own (TEX-039)
        if let Some(b) = wrappers.iter().find(|b| b.is_edge(i)) {
            close!();
            out.push(if b.open == i { Block::AlignStart(b.align) } else { Block::AlignEnd(b.align) });
            i += 1;
            continue;
        }
        // a formula on lines of its own (a `$$` block, a `math` fence): math when Shale can typeset it, else its text as code
        if let Some(m) = maths.iter().find(|m| m.block && m.lines.start == i) {
            close!();
            let tex = text[m.tex.clone()].trim();
            // the label of the equation stands on the closing line (or the opening line of a fence)
            let (from, to) = (lines[m.lines.start].0, lines[m.lines.end - 1].0 + lines[m.lines.end - 1].1.len());
            let id = equation_labels.iter().find(|(r, _)| r.start >= from && r.end <= to).map(|(_, id)| id.clone());
            out.push(if crate::math::typeset(tex, 14.0, true).is_err() {
                Block::Code { lang: String::new(), lines: lines[m.lines.clone()].iter().map(|l| l.1.to_string()).collect() }
            } else if let Some(id) = id {
                Block::Equation { tex: tex.to_string(), id }
            } else {
                Block::Math(tex.to_string())
            });
            i = m.lines.end.max(i + 1);
            continue;
        }
        // a footnote definition is written where its reference is (TEX-029)
        if let Some(d) = notes.def_at(i) {
            close!();
            i = d.lines.end;
            continue;
        }
        if let Some(b) = tables.iter().find(|b| b.lines.start == i) {
            close!();
            i = b.lines.end;
            match lines.get(i).and_then(|l| table_caption(l.1)) {
                Some((caption, id)) => {
                    out.push(Block::CaptionedTable { table: b.clone(), caption, id });
                    i += 1;
                }
                None => out.push(Block::Table(b.clone())),
            }
            continue;
        }
        let line = lines[i].1;
        let trimmed = line.trim();
        if page_breaks.contains(&i) {
            close!();
            out.push(Block::PageBreak);
            i += 1;
            continue;
        }
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
        } else if let Some(id) = links.first().filter(|l| links.len() == 1 && l.embed).and_then(|l| figure_label(line, l)) {
            close!();
            out.push(Block::Figure { link: links[0].clone(), id });
        } else if let Some(marker) = Marker::of_line(trimmed) {
            close!();
            out.push(Block::Marker(marker));
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

/// The caption line below a table: `: The caption {#tbl:runs}` gives the caption and the id (ADR-0039 §2).
fn table_caption(line: &str) -> Option<(String, String)> {
    let rest = line.trim().strip_prefix(':')?;
    let at = rest.rfind("{#")?;
    let (kind, id, len) = markdown::citations::parse_label(&rest[at..])?;
    (kind == markdown::citations::Kind::Table && at + len == rest.len()).then(|| (rest[..at].trim().to_string(), id))
}

/// The label after an embed that stands first on its line: `![[a.png|Caption]]{#fig:x}`, with no or
/// one space between (ADR-0039 §2).
fn figure_label(line: &str, link: &markdown::LinkSpan) -> Option<String> {
    let lead = line.len() - line.trim_start().len();
    if link.range.start != lead {
        return None;
    }
    let rest = line[link.range.end..].trim_end();
    let rest = rest.strip_prefix(' ').unwrap_or(rest);
    let (kind, id, len) = markdown::citations::parse_label(rest)?;
    (kind == markdown::citations::Kind::Figure && len == rest.len()).then_some(id)
}

/// The footnotes a note defines: label → the text of the definition, its first line after the label
/// and the lines that go on with it joined by spaces (TEX-029). The first definition of a label counts.
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
