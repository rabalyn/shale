//! A board through a LaTeX template (BOARD-031, #605): the board on real landscape pages ([`PREAMBLE`]),
//! the lanes as columns of cards, then the notes of the cards as the book of BOOK-013…BOOK-016 has
//! them (`book.rs`): a part for each lane that has cards, the cards as its chapters in the order of
//! [`Resolved::chapters`], with the contents. [`crate::pdf::board::plan`] is the reading order, shared
//! with the built-in engine, so both make the same book; this file writes the pages of the board.
//!
//! **Pages of the board.** Three lanes at most are one group, a `longtable` with a column for each
//! lane and a row for each card index; more lanes make more groups, one after the other, each on
//! pages of its own. LaTeX breaks the table where the page is full (rows are never split, so a card
//! never is), and the head of the table says "(continued)" on the pages after the first, as in the
//! built-in export. A card is a framed box with its title (a link to its chapter) and its chips; a
//! drawing card is its title, its drawing is the chapter. A lane whose query fails has the reason
//! under its name.

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

/// What the document needs for the board's landscape pages (BOARD-031, #641): `\shaleLandscape`
/// ends the page and makes the paper landscape with the margins, header and footer of the class
/// (the page box is landscape, so the template's header bar and page number stay at the top and
/// bottom edge); `\shalePortrait` ends the page and brings the portrait layout back. One way for
/// every class: `geometry` (the class's own, as tudapub loads it, else loaded here with `pass`, which
/// leaves the layout alone) with a `\newgeometry` of the margins the portrait pages have, on the
/// turned paper. `geometry` cannot change the paper itself, so the sizes of the paper and the page
/// box are swapped here. KOMA-Script keeps the width of the header and footer as a length, which
/// is set again to the new text width and back. (`typearea`'s `paper=landscape` was tried: for
/// tudapub it recomputes the type area to a fraction of the page.)
pub(super) const PREAMBLE: &str = "\\IfPackageLoadedTF{geometry}{}{\\usepackage[pass]{geometry}}
\\makeatletter
\\newdimen\\shale@pw \\newdimen\\shale@ph \\newdimen\\shale@l \\newdimen\\shale@t \\newdimen\\shale@w \\newdimen\\shale@h
\\newcommand*\\shale@paper{\\pagewidth=\\paperwidth \\pageheight=\\paperheight\\relax}
\\newcommand*\\shaleLandscape{\\clearpage
  \\savegeometry{shale-portrait}%
  \\ifdefined\\sls@headwidth \\let\\shale@hw\\sls@headwidth \\let\\shale@fw\\sls@footwidth \\fi
  \\shale@pw=\\paperwidth \\shale@ph=\\paperheight
  \\shale@l=\\dimexpr1in+\\hoffset+\\oddsidemargin\\relax
  \\shale@t=\\dimexpr1in+\\voffset+\\topmargin\\relax
  \\shale@w=\\dimexpr\\shale@ph-\\shale@l-\\shale@l\\relax
  \\shale@h=\\dimexpr\\shale@pw-\\shale@ph+\\headheight+\\headsep+\\textheight\\relax
  \\edef\\shale@geometry{includehead,nomarginpar,left=\\the\\shale@l,width=\\the\\shale@w,top=\\the\\shale@t,height=\\the\\shale@h,headheight=\\the\\headheight,headsep=\\the\\headsep,footskip=\\the\\footskip}%
  \\expandafter\\newgeometry\\expandafter{\\shale@geometry}%
  \\paperwidth=\\shale@ph \\paperheight=\\shale@pw \\shale@paper
  \\ifdefined\\sls@headwidth \\KOMAoptions{headwidth=text,footwidth=text}\\fi}
\\newcommand*\\shalePortrait{\\clearpage
  \\loadgeometry{shale-portrait}\\paperwidth=\\shale@pw \\paperheight=\\shale@ph \\shale@paper
  \\ifdefined\\shale@hw \\let\\sls@headwidth\\shale@hw \\let\\sls@footwidth\\shale@fw \\fi}
\\makeatother
";

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
}

fn boxed(card: &Card, shows: &[Chip], targets: &Targets) -> Boxed {
    Boxed { title: card.title.clone(), chips: card.chip_texts(shows), anchor: targets.notes.get(&card.rel).map(|(a, _)| a.clone()) }
}

fn card(c: &Boxed) -> String {
    let title = tex_escape(&c.title);
    let title = match &c.anchor {
        Some(a) => format!("\\hyperlink{{{a}}}{{{title}}}"),
        None => title,
    };
    let chips = if c.chips.is_empty() { String::new() } else { format!("\\par{{\\footnotesize {}}}", c.chips.iter().map(|t| tex_escape(t)).collect::<Vec<_>>().join(" \\textperiodcentered{} ")) };
    format!("\\noindent\\fbox{{\\begin{{minipage}}{{\\dimexpr\\linewidth-2\\fboxsep-2\\fboxrule\\relax}}\\raggedright\\bfseries {title}{chips}\\end{{minipage}}}}\\par\\smallskip")
}

/// The head of a lane's column: the name with the number of cards, and on the first head the reason
/// of a failed query under it.
fn head(lane: &LaneView, cont: bool) -> String {
    let more = if cont { " (continued)" } else { "" };
    let mut out = format!("{{\\bfseries {}{more}}}\\hfill{{\\footnotesize {}}}\\par\\nointerlineskip\\vspace{{2pt}}\\hrule\\medskip", tex_escape(&lane.name), lane.cards.len());
    if let (false, Some(why)) = (cont, &lane.error) {
        out.push_str(&format!("\\par{{\\footnotesize\\itshape {}}}", tex_escape(&format!("Query: {why}"))));
    }
    out
}

/// A group of lanes (at most [`LANES_PER_PAGE`]) as one `longtable` with a column for each lane (BOARD-031):
/// row `i` has the `i`-th card of each lane, and LaTeX breaks the table where the page is full, whole
/// rows only, so no height is estimated. The heads (the board's title and the lanes' names) are the
/// table's first head, and its head on every following page with "(continued)".
fn table(title: &str, group: &[LaneView], cards: &[Vec<Boxed>], continued: bool) -> String {
    let n = group.len();
    let spec = (0..n).map(|_| format!("p{{{COLUMN_SHARE}\\linewidth}}")).collect::<Vec<_>>().join(&format!("@{{\\hspace{{{GAP_SHARE}\\linewidth}}}}"));
    let heads = |cont: bool| {
        let title = format!("\\multicolumn{{{n}}}{{@{{}}l@{{}}}}{{\\large\\bfseries {}{}}}\\\\[\\medskipamount]\n", tex_escape(title), if cont { " (continued)" } else { "" });
        format!("{title}{}\\\\\n", group.iter().map(|l| head(l, cont)).collect::<Vec<_>>().join(" &\n"))
    };
    let mut out = format!("\\begin{{longtable}}{{@{{}}{spec}@{{}}}}\n{}\\endfirsthead\n{}\\endhead\n", heads(continued), heads(true));
    let rows = cards.iter().map(Vec::len).max().unwrap_or(0);
    for r in 0..rows {
        let cells: Vec<String> = cards.iter().map(|c| c.get(r).map(card).unwrap_or_default()).collect();
        out.push_str(&format!("{}\\\\\n", cells.join(" &\n")));
    }
    if rows == 0 {
        // a table needs a row to end on
        out.push_str(&format!("{}\\\\\n", vec![""; n].join(" & ")));
    }
    out.push_str("\\end{longtable}\n");
    out
}

/// The pages of the board (BOARD-031): landscape, the lanes as columns of a table. `listed` is the rank of
/// the sectioning command the board has in the contents when there are some.
pub(super) fn pages(board: &Resolved, targets: &Targets, listed: Option<usize>) -> String {
    let mut out = String::from("\\shaleLandscape\n");
    if let Some(rank) = listed {
        out.push_str(&format!("\\phantomsection\\addcontentsline{{toc}}{{{}}}{{{}}}\n", COMMANDS[rank.min(COMMANDS.len() - 1)], tex_escape(&board.title)));
    }
    if board.lanes.is_empty() {
        out.push_str(&format!("\\noindent{{\\large\\bfseries {}}}\\par\\medskip\n\\noindent This board has no lanes.\\par\n", tex_escape(&board.title)));
    }
    // the groups of lanes one after the other, each on pages of its own
    for (g, group) in board.lanes.chunks(LANES_PER_PAGE).enumerate() {
        if g > 0 {
            out.push_str("\\clearpage\n");
        }
        let cards: Vec<Vec<Boxed>> = group.iter().map(|l| l.cards.iter().map(|c| boxed(c, &board.props.shows, targets)).collect()).collect();
        out.push_str(&table(&board.title, group, &cards, g > 0));
    }
    out.push_str("\\shalePortrait\n");
    out
}
