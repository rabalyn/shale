//! The order of the cards of a lane (BOARD-015…BOARD-018). The `sort:` line is read and written
//! by [`crate::search_form::sort`], the one reader of the form and the board; this file compares
//! the cards by its keys.
//!
//! The pinned cards stay on top in the order of their links (BOARD-022); the others are ordered
//! by the keys. A card without a value for a key comes after the others in either direction
//! (BOARD-017). The key `dependencies` is read but orders nothing yet (BOARD-026, #603).

use super::card::Card;
use crate::search_form::sort::{Field, Key};
use std::cmp::Ordering;

/// Compare two optional values: a missing one is last, whatever the direction (BOARD-017).
fn by<T: Ord>(a: Option<T>, b: Option<T>, desc: bool) -> Ordering {
    match (a, b) {
        (Some(a), Some(b)) if desc => b.cmp(&a),
        (Some(a), Some(b)) => a.cmp(&b),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn titles(a: &Card, b: &Card) -> Ordering {
    a.title.to_lowercase().cmp(&b.title.to_lowercase())
}

/// Order `cards` by `keys` in order; the cards the keys do not tell apart, and all of them
/// without keys, come by title (BOARD-018).
pub fn sort(cards: &mut [Card], keys: &[Key]) {
    cards.sort_by(|a, b| {
        for key in keys {
            let order = match key.field {
                Field::Created => by(a.created, b.created, key.desc),
                Field::Modified => by(a.modified, b.modified, key.desc),
                Field::Due => by(a.due, b.due, key.desc),
                Field::Priority => by(a.priority, b.priority, key.desc),
                Field::Title if key.desc => titles(b, a),
                Field::Title => titles(a, b),
                Field::Dependencies => Ordering::Equal,
            };
            if order != Ordering::Equal {
                return order;
            }
        }
        titles(a, b).then_with(|| a.rel.cmp(&b.rel))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::note_props::Priority;
    use crate::search_form::sort::parse;
    use std::path::PathBuf;

    fn card(title: &str) -> Card {
        Card { rel: PathBuf::from(format!("{title}.md")), title: title.into(), drawing: false, due: None, status: None, priority: None, tags: vec![], created: None, modified: None }
    }

    fn order(cards: &mut [Card], keys: &str) -> Vec<String> {
        sort(cards, &parse(keys).unwrap());
        cards.iter().map(|c| c.title.clone()).collect()
    }

    /// Delta has no value at all, Charlie no `modified`.
    fn deck() -> Vec<Card> {
        let mut a = card("Alpha");
        let mut b = card("bravo");
        let mut c = card("Charlie");
        let d = card("Delta");
        (a.priority, a.due, a.created, a.modified) = (Some(Priority::Low), Some(20), Some(300), Some(5));
        (b.priority, b.due, b.created, b.modified) = (Some(Priority::Highest), Some(10), Some(100), Some(9));
        (c.priority, c.due, c.created, c.modified) = (Some(Priority::Highest), Some(30), Some(200), None);
        vec![d, c, a, b]
    }

    // spec: BOARD-015
    #[test]
    fn board_015_the_keys_sort_in_order_with_a_direction() {
        let mut cards = deck();
        assert_eq!(order(&mut cards, "priority desc, due asc"), ["bravo", "Charlie", "Alpha", "Delta"]);
        assert_eq!(order(&mut cards, "priority asc, due desc"), ["Alpha", "Charlie", "bravo", "Delta"]);
        assert_eq!(order(&mut cards, "due"), ["bravo", "Alpha", "Charlie", "Delta"], "asc is the default");
        assert_eq!(order(&mut cards, "created desc"), ["Alpha", "Charlie", "bravo", "Delta"]);
        assert_eq!(order(&mut cards, "modified asc"), ["Alpha", "bravo", "Charlie", "Delta"]);
        assert_eq!(order(&mut cards, "title desc"), ["Delta", "Charlie", "bravo", "Alpha"], "titles compare without case");
    }

    // spec: BOARD-016
    #[test]
    fn board_016_each_key_compares_its_own_value() {
        let mut cards = deck();
        assert_eq!(order(&mut cards, "title"), ["Alpha", "bravo", "Charlie", "Delta"]);
        assert_eq!(order(&mut cards, "priority desc"), ["bravo", "Charlie", "Alpha", "Delta"], "a tie goes by title");
    }

    // spec: BOARD-017
    #[test]
    fn board_017_a_card_without_a_value_is_last_in_either_direction() {
        for keys in ["priority asc", "priority desc", "due asc", "due desc", "created asc", "created desc", "modified asc", "modified desc"] {
            let mut cards = deck();
            let titles = order(&mut cards, keys);
            assert_eq!(titles[3], "Delta", "{keys}: {titles:?}");
            if keys.starts_with("modified") {
                assert_eq!(titles[2], "Charlie", "{keys}: {titles:?}");
            }
        }
    }

    // spec: BOARD-018
    #[test]
    fn board_018_without_keys_the_cards_come_by_title() {
        let mut cards = deck();
        assert_eq!(order(&mut cards, ""), ["Alpha", "bravo", "Charlie", "Delta"]);
        // the dependencies key orders nothing yet: the title decides
        assert_eq!(order(&mut cards, "dependencies"), ["Alpha", "bravo", "Charlie", "Delta"]);
    }
}
