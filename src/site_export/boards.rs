//! The snapshot of a board note (SITE-045): its lanes and cards as they are when the site is
//! exported, resolved by the board's own resolver ([`crate::board::resolve`], so the order and the
//! chips are the view's), written to `data/boards.json` for the theme's `board.html` to lay out.
//!
//! A card of a note that is not published is **left out**: not counted, not named, so the snapshot
//! cannot tell that it exists (SITE-007, SITE-010). Nothing of the lanes' definitions is written
//! either (queries, pins, sorts), nor the reasons a lane failed: they could name unpublished notes.

use super::body::{self, Env};
use crate::board::card::Card;
use crate::board::deps::Dep;
use crate::board::file::{self, Board};
use crate::board::resolve::resolve;
use crate::vault::rel_to_string;
use serde_json::{json, Value};
use std::path::Path;

/// Whether the note with this text is a board.
pub fn is_board(text: &str) -> bool {
    file::is_board(text)
}

/// The board's own words, the text of the page: what is above the first lane (the lanes
/// themselves are the snapshot, SITE-045).
pub fn intro(text: &str) -> String {
    Board::parse(text).intro()
}

/// The tags of a board page: those of its front matter and of its own words (`intro`). A lane's
/// query line such as `query: #hushtag` makes the vault tag the note, but that tag is the lane's
/// filter, and it could belong to notes that are not published (SITE-010).
pub fn tags(text: &str, intro: &str) -> Vec<String> {
    let mut tags = crate::markdown::frontmatter_tags(text);
    tags.extend(crate::markdown::find_tags(intro).into_iter().map(|t| t.1));
    tags.sort();
    tags.dedup();
    tags
}

/// The snapshot of the published board `rel` and what is worth a warning (SITE-067).
pub fn snapshot(env: &Env, rel: &Path) -> (Value, Vec<String>) {
    let name = rel_to_string(rel);
    let board = match resolve(env.vault, rel) {
        Ok(b) => b,
        Err(e) => return (json!({"unresolved": true}), vec![format!("{name}: the board could not be resolved ({e}); its page shows a placeholder")]),
    };
    let mut warnings = Vec::new();
    let lanes: Vec<Value> = board
        .lanes
        .iter()
        .map(|lane| {
            let title = body::flatten_links(env, rel, &lane.name);
            if let Some(reason) = &lane.error {
                warnings.push(format!("{name}: lane “{}” could not be resolved ({reason}); its page shows a placeholder", lane.name));
                return json!({"name": title, "unresolved": true, "cards": []});
            }
            if lane.partial {
                warnings.push(format!("{name}: lane “{}” was cut short by the search's work budget; its page says it may be incomplete", lane.name));
            }
            let cards: Vec<Value> = lane
                .cards
                .iter()
                .filter_map(|card| {
                    // a card of a note that is not published is not there at all
                    let place = env.layout.notes.get(&card.rel)?;
                    let title = env.titles.get(&card.rel)?;
                    // (the dependency chips depend on notes that may be unpublished: left out)
                    let plain = Card { dep: Dep::default(), ..card.clone() };
                    Some(json!({"title": title, "url": place.url, "chips": plain.chip_texts(&board.props.shows)}))
                })
                .collect();
            json!({"name": title, "partial": lane.partial, "cards": cards})
        })
        .collect();
    (json!({"lanes": lanes}), warnings)
}
