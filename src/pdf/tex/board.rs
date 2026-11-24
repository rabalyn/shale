//! A board through a LaTeX template (BOARD-031, #605): the board on landscape pages (`pdflscape`),
//! the lanes as columns of cards, then the notes of the cards as the book of BOOK-013…BOOK-016 has
//! them (`book.rs`): a part for each lane that has cards, the cards as its chapters in the order of
//! [`Resolved::chapters`], with the contents. [`crate::pdf::board::plan`] is the reading order, shared
//! with the built-in engine, so both make the same book; this file writes the pages of the board.
//!
//! **Pages of the board.** Three lanes at most are one group of columns (`minipage`s side by side);
//! more lanes make more groups, one after the other. LaTeX cannot ask how high a box is before it
//! is set, so the height of a card is estimated from the characters of its lines
//! ([`estimate`]) and the cards that do not fit the page go on the next landscape page of the group,
//! whose header says "(continued)", as in the built-in export. A card is never split. A card is a
//! framed box with its title (a link to its chapter) and its chips; a drawing card is its title,
//! its drawing is the chapter. A lane whose query fails has the reason under its name.

use super::book::{write_with_board, BookRequest};
use super::inline::Targets;
use super::sets::{tex_escape, KindManifest, SetInfo};
use super::writer::{Document, COMMANDS};
use crate::board::card::Card;
use crate::board::file::Chip;
use crate::board::resolve::Resolved;
use crate::board::LaneView;
use crate::pdf::export::ExportOptions;
use crate::vault::Vault;
use std::collections::BTreeMap;

/// How many lanes are side by side on a page, and the share of the line each of them has (with
/// the two gaps of 2 % this is the whole line).
const LANES_PER_PAGE: usize = 3;
const COLUMN_SHARE: &str = "0.32";
const GAP_SHARE: &str = "0.02";
/// What fits a page of a column, in lines of text: the estimate of [`estimate`] is against this.
const PAGE_LINES: f32 = 22.0;
/// The characters of a line of a title (bold) and of the chips (small) in a column.
const TITLE_CHARS: usize = 28;
const CHIP_CHARS: usize = 44;

/// The board and the choices it is written with.
pub struct BoardRequest<'a> {
    pub vault: &'a Vault,
    pub board: &'a Resolved,
    pub opts: &'a ExportOptions,
    pub set: &'a SetInfo,
    pub kind: &'a KindManifest,
    /// The values of the dialog's fields by key; an empty one is as good as none.
    pub fields: &'a BTreeMap<String, String>,
}

/// Write the board as one document of the kind (BOARD-031): the title, the board on landscape
/// pages, the contents, the cards' notes in parts and chapters. Fails with a sentence for the person
/// when the kind cannot hold a book or a required field has no value.
pub fn write_board(req: &BoardRequest) -> Result<Document, String> {
    let plan = crate::pdf::board::plan(req.vault, req.board);
    write_with_board(&BookRequest { vault: req.vault, plan: &plan, opts: req.opts, set: req.set, kind: req.kind, fields: req.fields }, Some(req.board))
}

/// A card as it is set: its title, the chips and the anchor of its chapter when it has one.
struct Boxed {
    title: String,
    chips: Vec<String>,
    anchor: Option<String>,
    height: f32,
}

/// The lines a card takes on a page, about: its title, its chips and the frame around them.
fn estimate(title: &str, chips: &[String]) -> f32 {
    let chips = chips.join("  ");
    let lines = |t: &str, per: usize| t.chars().count().div_ceil(per).max(1) as f32;
    lines(title, TITLE_CHARS) + if chips.is_empty() { 0.0 } else { 0.85 * lines(&chips, CHIP_CHARS) } + 0.9
}

fn boxed(card: &Card, shows: &[Chip], targets: &Targets) -> Boxed {
    let chips = card.chip_texts(shows);
    let height = estimate(&card.title, &chips);
    Boxed { title: card.title.clone(), chips, anchor: targets.notes.get(&card.rel).map(|(a, _)| a.clone()), height }
}

/// How the cards of a column are spread over the pages that follow each other: the number of cards
/// on each page, at least one page. `used` is what the page has already (the reason of a failed query).
fn spread(cards: &[Boxed], used: f32) -> Vec<usize> {
    let mut pages = vec![0usize];
    let mut taken = used;
    for c in cards {
        if taken + c.height > PAGE_LINES && pages.last().is_some_and(|n| *n > 0) {
            pages.push(0);
            taken = 0.0;
        }
        taken += c.height;
        if let Some(n) = pages.last_mut() {
            *n += 1;
        }
    }
    pages
}

/// The lines the reason of a failed query takes under the lane's name.
fn error_lines(why: &str) -> f32 {
    (why.chars().count() + "Query: ".len()).div_ceil(CHIP_CHARS) as f32 * 0.85 + 0.3
}

fn card(c: &Boxed) -> String {
    let title = tex_escape(&c.title);
    let title = match &c.anchor {
        Some(a) => format!("\\hyperlink{{{a}}}{{{title}}}"),
        None => title,
    };
    let chips = if c.chips.is_empty() { String::new() } else { format!("\\par{{\\footnotesize {}}}", c.chips.iter().map(|t| tex_escape(t)).collect::<Vec<_>>().join(" \\textperiodcentered{} ")) };
    format!("\\noindent\\fbox{{\\begin{{minipage}}{{\\dimexpr\\linewidth-2\\fboxsep-2\\fboxrule\\relax}}\\raggedright\\bfseries {title}{chips}\\end{{minipage}}}}\\par\\smallskip\n")
}

/// The column of a lane on page `p` of its group: the name with the number of cards, the reason
/// of a failed query (first page), the cards of that page.
fn column(lane: &LaneView, cards: &[Boxed], p: usize) -> String {
    let cont = if p > 0 { " (continued)" } else { "" };
    let mut out = format!("\\begin{{minipage}}[t]{{{COLUMN_SHARE}\\linewidth}}\n{{\\bfseries {}{cont}}}\\hfill{{\\footnotesize {}}}\\par\\nointerlineskip\\vspace{{2pt}}\\hrule\\medskip\n", tex_escape(&lane.name), lane.cards.len());
    if let (0, Some(why)) = (p, &lane.error) {
        out.push_str(&format!("{{\\footnotesize\\itshape {}}}\\par\\smallskip\n", tex_escape(&format!("Query: {why}"))));
    }
    for c in cards {
        out.push_str(&card(c));
    }
    out.push_str("\\end{minipage}");
    out
}

/// The pages of the board (BOARD-031): landscape, the lanes as columns. `listed` is the rank of
/// the sectioning command the board has in the contents when there are some.
pub(super) fn pages(board: &Resolved, targets: &Targets, listed: Option<usize>) -> String {
    let mut out = String::new();
    let groups: Vec<&[LaneView]> = if board.lanes.is_empty() { vec![&[]] } else { board.lanes.chunks(LANES_PER_PAGE).collect() };
    let mut first = true;
    for (g, group) in groups.iter().enumerate() {
        let cards: Vec<Vec<Boxed>> = group.iter().map(|l| l.cards.iter().map(|c| boxed(c, &board.props.shows, targets)).collect()).collect();
        let spreads: Vec<Vec<usize>> = group.iter().zip(&cards).map(|(l, c)| spread(c, l.error.as_deref().map_or(0.0, error_lines))).collect();
        let count = spreads.iter().map(Vec::len).max().unwrap_or(1);
        let mut taken = vec![0usize; group.len()];
        for p in 0..count {
            out.push_str("\\begin{landscape}\n");
            if first {
                if let Some(rank) = listed {
                    out.push_str(&format!("\\phantomsection\\addcontentsline{{toc}}{{{}}}{{{}}}\n", COMMANDS[rank.min(COMMANDS.len() - 1)], tex_escape(&board.title)));
                }
                first = false;
            }
            let cont = if g == 0 && p == 0 { "" } else { " (continued)" };
            out.push_str(&format!("\\noindent{{\\large\\bfseries {}{cont}}}\\par\\medskip\n", tex_escape(&board.title)));
            if group.is_empty() {
                out.push_str("\\noindent This board has no lanes.\\par\n");
            }
            let row: Vec<String> = group
                .iter()
                .enumerate()
                .map(|(i, lane)| {
                    let n = spreads[i].get(p).copied().unwrap_or(0);
                    let col = column(lane, &cards[i][taken[i]..taken[i] + n], p);
                    taken[i] += n;
                    col
                })
                .collect();
            if !row.is_empty() {
                out.push_str("\\noindent");
                out.push_str(&row.join(&format!("\\hspace{{{GAP_SHARE}\\linewidth}}%\n")));
                out.push('\n');
            }
            out.push_str("\\end{landscape}\n");
        }
    }
    out
}
