//! The PDF of a board, built-in engine (BOARD-030, #604): the board on landscape pages with the
//! lanes as columns, then the notes of its cards on portrait pages through the book export
//! (BOOK-005…BOOK-009), lane by lane and card by card, with a table of contents whose first line is
//! the board. [`crate::board::resolve`] says which lanes and cards there are; this file makes the
//! reading order of the notes ([`plan`]) and draws the board pages ([`draw_pages`]).
//!
//! **Pages of the board.** The columns that fit the width of a landscape page are one group; more
//! lanes make more groups, which follow each other left to right: all pages of the first group,
//! then those of the next. When the cards of a column of a group do not fit the height of the
//! page, the group goes on at the next page with the same columns (the header says "continued"),
//! the cards going on where they stopped. A card is never cut in two. A card shows its title and
//! the chips `card-shows:` chooses; a drawing card also shows its drawing as a vector picture
//! (PDF-001). A lane whose query fails the check has its reason as text under its name.

use crate::board::card::Card;
use crate::board::resolve::{resolve, Resolved};
use crate::drawing::Scene;
use crate::pdf::book::{self, Chapter, Item, Part, Plan};
use crate::pdf::book_export::{lay_out, Progress};
use crate::pdf::export::{add_pictures, draw_scene, fit_text, wrap_pieces, ExportOptions, Piece, Writer};
use crate::pdf::export_fonts::Made;
use crate::pdf::write::{FontKind, Rgba, TextStyle};
use crate::vault::{FileKind, Vault};
use std::path::{Path, PathBuf};

const BLACK: Rgba = Rgba(0, 0, 0, 255);
const GREY: Rgba = Rgba(110, 110, 115, 255);
const RED: Rgba = Rgba(185, 45, 45, 255);
const CARD_FILL: Rgba = Rgba(247, 247, 250, 255);
const CARD_LINE: Rgba = Rgba(200, 200, 208, 255);

/// The width a column is at most, and the least room between two.
const MAX_COLUMN: f32 = 260.0;
const NARROW_COLUMN: f32 = 190.0;
const GAP: f32 = 10.0;
/// The band at the top of a page of the board, the header of a lane and the room around a card.
const BAND: f32 = 26.0;
const LANE_HEAD: f32 = 24.0;
const PAD: f32 = 6.0;
const TITLE_SIZE: f32 = 10.0;
const CHIP_SIZE: f32 = 8.0;
/// The picture of a drawing card, and how many lines of a card's text are printed.
const THUMB: f32 = 60.0;
const MAX_LINES: usize = 4;

/// What was drawn: the first page of the board and the place of every card, for the links to
/// their notes.
pub(super) struct Drawn {
    pub(super) first: usize,
    pub(super) cards: Vec<(usize, [f32; 4], PathBuf)>,
}

/// The notes of a board in reading order as a plan: a part for each lane that has cards, its
/// cards as the chapters (BOARD-030). A card of several lanes is in the first one only.
pub fn plan(vault: &Vault, board: &Resolved) -> Plan {
    let items = board
        .chapters()
        .into_iter()
        .filter_map(|(lane, cards)| {
            let chapters: Vec<Item> = cards.into_iter().map(|c| Item::Chapter(chapter(vault, c))).collect();
            chapters.iter().any(|i| matches!(i, Item::Chapter(c) if !c.left_out)).then(|| Item::Part(Part { dir: PathBuf::new(), title: lane.name.clone(), note: None, items: chapters }))
        })
        .collect();
    Plan { dir: PathBuf::new(), title: board.title.clone(), subtitle: None, author: None, date: None, logo: None, intro: None, note: None, items }
}

/// A card as a chapter: titled as a note of a book is (BOOK-004), left out with `export: false`.
fn chapter(vault: &Vault, card: &Card) -> Chapter {
    let text = vault.notes.get(&card.rel).map(|n| n.text.as_str()).unwrap_or_default();
    Chapter { rel: card.rel.clone(), title: book::property(text, "title").unwrap_or_else(|| card.title.clone()), left_out: book::left_out(text) }
}

/// The reading order of the board `rel`, or why there is no board.
pub fn plan_of(vault: &Vault, rel: &Path) -> Result<Plan, String> {
    resolve(vault, rel).map(|b| plan(vault, &b))
}

/// The PDF of the board note `rel`: the board, then its cards' notes (BOARD-030). `progress`
/// counts the notes laid out and can stop it.
pub fn export_board(vault: &Vault, rel: &Path, opts: &ExportOptions, progress: Option<&Progress>) -> Result<Made, String> {
    let _flow = tracing::info_span!("flow.board_export", peer = "export").entered();
    let board = tracing::info_span!("flow.board_export.resolve", peer = "vault").in_scope(|| resolve(vault, rel))?;
    lay_out(vault, &plan(vault, &board), opts, progress, Some(&board))
}

/// One card as it is drawn: its lines of text and what it needs.
struct Boxed<'a> {
    card: &'a Card,
    title: Vec<String>,
    chips: Vec<String>,
    scene: Option<Scene>,
    height: f32,
}

fn lines_of(text: &str, size: f32, bold: bool, width: f32) -> Vec<String> {
    let mut lines: Vec<String> = wrap_pieces(&[Piece::plain(text, size, BLACK, bold)], width).iter().map(|l| l.iter().map(|p| p.text.as_str()).collect::<String>().trim_end().to_string()).collect();
    if lines.len() > MAX_LINES {
        lines.truncate(MAX_LINES);
        if let Some(last) = lines.last_mut() {
            *last = fit_text(FontKind::Sans, &format!("{last}…"), size, width);
        }
    }
    lines
}

fn measure<'a>(vault: &Vault, card: &'a Card, shows: &[crate::board::file::Chip], width: f32) -> Boxed<'a> {
    let inner = width - 2.0 * PAD;
    let title = lines_of(&card.title, TITLE_SIZE, true, inner);
    let chips = card.chip_texts(shows);
    let chips = if chips.is_empty() { Vec::new() } else { lines_of(&chips.join("  ·  "), CHIP_SIZE, false, inner) };
    let scene = (card.drawing && vault.file(&card.rel).is_some_and(|f| f.kind == FileKind::Drawing)).then(|| Scene::read_file(&vault.abs(&card.rel)).ok()).flatten().filter(|s| s.bounds().is_some());
    let height = 2.0 * PAD + title.len() as f32 * TITLE_SIZE * 1.3 + if scene.is_some() { THUMB + PAD } else { 0.0 } + if chips.is_empty() { 0.0 } else { 3.0 + chips.len() as f32 * CHIP_SIZE * 1.3 };
    Boxed { card, title, chips, scene, height }
}

/// How the cards of a column are spread over the pages that follow each other: the number of
/// cards on each page, at least one page.
fn spread(cards: &[Boxed], room: f32) -> Vec<usize> {
    let mut pages = vec![0usize];
    let mut used = 0.0;
    for c in cards {
        if used + c.height > room && *pages.last().unwrap_or(&0) > 0 {
            pages.push(0);
            used = 0.0;
        }
        used += c.height + PAD;
        if let Some(n) = pages.last_mut() {
            *n += 1;
        }
    }
    pages
}

/// Draw the board on landscape pages of the writer's paper (BOARD-030).
pub(super) fn draw_pages(w: &mut Writer, vault: &Vault, board: &Resolved) -> Drawn {
    let [a, b] = w.opts.page_size();
    let (pw, ph) = (a.max(b), a.min(b));
    let width = pw - w.left - w.right;
    let fit = (((width + GAP) / (NARROW_COLUMN + GAP)).floor() as usize).max(1);
    let per_group = fit.min(board.lanes.len()).max(1);
    let column = ((width - (per_group - 1) as f32 * GAP) / per_group as f32).min(MAX_COLUMN);
    let top = w.top + BAND;
    let room = ph - w.bottom - top - LANE_HEAD - PAD;
    let mut drawn = Drawn { first: 0, cards: Vec::new() };
    let mut first = true;
    let groups: Vec<&[crate::board::LaneView]> = if board.lanes.is_empty() { vec![&[]] } else { board.lanes.chunks(per_group).collect() };
    for (g, group) in groups.iter().enumerate() {
        let columns: Vec<Vec<Boxed>> = group.iter().map(|l| l.cards.iter().map(|c| measure(vault, c, &board.props.shows, column)).collect()).collect();
        let spreads: Vec<Vec<usize>> = columns.iter().map(|c| spread(c, room)).collect();
        let pages = spreads.iter().map(Vec::len).max().unwrap_or(1);
        let mut taken = vec![0usize; columns.len()];
        for p in 0..pages {
            let page = w.sized_page(pw, ph, first);
            if first {
                drawn.first = page;
                first = false;
            }
            let head = if g == 0 && p == 0 { board.title.clone() } else { format!("{} (continued)", board.title) };
            let band = fit_text(FontKind::Sans, &head, 13.0, width);
            w.doc.page(page).text(TextStyle { bold: true, ..TextStyle::plain(FontKind::Sans, 13.0, BLACK) }, w.left, w.top + 13.0, &band);
            if group.is_empty() {
                w.doc.page(page).text(TextStyle::plain(FontKind::Sans, 10.0, GREY), w.left, top + 12.0, "This board has no lanes.");
            }
            for (i, lane) in group.iter().enumerate() {
                let x = w.left + i as f32 * (column + GAP);
                let cont = if p > 0 { " (continued)" } else { "" };
                let name = fit_text(FontKind::Sans, &format!("{}{cont}", lane.name), 11.0, column - 30.0);
                let count = lane.cards.len().to_string();
                let pg = w.doc.page(page);
                pg.text(TextStyle { bold: true, ..TextStyle::plain(FontKind::Sans, 11.0, BLACK) }, x, top + 12.0, &name);
                pg.text(TextStyle::plain(FontKind::Sans, 9.0, GREY), x + column - crate::pdf::write::text_width(FontKind::Sans, 9.0, &count), top + 12.0, &count);
                pg.line([x, top + LANE_HEAD - 6.0], [x + column, top + LANE_HEAD - 6.0], CARD_LINE, 0.8);
                let mut y = top + LANE_HEAD;
                if let (0, Some(why)) = (p, &lane.error) {
                    for line in lines_of(&format!("Query: {why}"), CHIP_SIZE, false, column) {
                        y += CHIP_SIZE * 1.3;
                        w.doc.page(page).text(TextStyle::plain(FontKind::Sans, CHIP_SIZE, RED), x, y, &line);
                    }
                }
                let count = spreads[i].get(p).copied().unwrap_or(0);
                for c in &columns[i][taken[i]..taken[i] + count] {
                    draw_card(w, vault, page, [x, y, column], c);
                    drawn.cards.push((page, [x, y, column, c.height], c.card.rel.clone()));
                    y += c.height + PAD;
                }
                taken[i] += count;
            }
        }
    }
    drawn
}

fn draw_card(w: &mut Writer, vault: &Vault, page: usize, [x, y, width]: [f32; 3], c: &Boxed) {
    // the pictures of a drawing are part of the document before the page uses them
    let pictures = c.scene.as_ref().map(|s| add_pictures(&mut w.doc, &crate::drawing::pictures::images(s, vault, &c.card.rel)));
    let pg = w.doc.page(page);
    pg.rect(x, y, width, c.height, Some(CARD_FILL), Some((CARD_LINE, 0.8)));
    let mut base = y + PAD;
    for line in &c.title {
        base += TITLE_SIZE * 1.3;
        pg.text(TextStyle { bold: true, ..TextStyle::plain(FontKind::Sans, TITLE_SIZE, BLACK) }, x + PAD, base - TITLE_SIZE * 0.3, line);
    }
    if let (Some(scene), Some(pictures)) = (&c.scene, &pictures) {
        base += 2.0;
        draw_scene(pg, scene, [x + PAD, base, width - 2.0 * PAD, THUMB], false, Some(Rgba(255, 255, 255, 255)), pictures);
        base += THUMB + 2.0;
    }
    if !c.chips.is_empty() {
        base += 3.0;
    }
    for line in &c.chips {
        base += CHIP_SIZE * 1.3;
        pg.text(TextStyle::plain(FontKind::Sans, CHIP_SIZE, GREY), x + PAD, base - CHIP_SIZE * 0.3, line);
    }
}
