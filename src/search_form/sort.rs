//! The sort of a lane as the form edits it (QUERY-023): the value of the `sort:` line of a board
//! file, `due asc, priority desc` (BOARD-015). A key is a field and a direction; `dependencies`
//! has none (BOARD-026). The text is written in one way (`due asc`, lower case, a comma and a
//! space between keys), and read in any case.

/// What a sort key looks at (BOARD-016).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Created,
    Modified,
    Due,
    Priority,
    Title,
    /// the order of the dependencies (BOARD-026): no direction
    Dependencies,
}

impl Field {
    pub const ALL: [Field; 6] = [Field::Created, Field::Modified, Field::Due, Field::Priority, Field::Title, Field::Dependencies];

    /// The word of the `sort:` line.
    pub fn word(self) -> &'static str {
        match self {
            Field::Created => "created",
            Field::Modified => "modified",
            Field::Due => "due",
            Field::Priority => "priority",
            Field::Title => "title",
            Field::Dependencies => "dependencies",
        }
    }

    /// Does the field have a direction?
    pub fn directed(self) -> bool {
        self != Field::Dependencies
    }

    fn of_word(word: &str) -> Option<Field> {
        Field::ALL.into_iter().find(|f| f.word().eq_ignore_ascii_case(word))
    }
}

/// One key of a sort: a field and, for the fields that have one, a direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key {
    pub field: Field,
    /// descending; always false for a field without a direction
    pub desc: bool,
}

impl Key {
    pub fn new(field: Field, desc: bool) -> Key {
        Key { field, desc: desc && field.directed() }
    }
}

/// Read the value of a `sort:` line: keys separated by commas, each a field and `asc` (the
/// default) or `desc`. A blank text is no key. The error says what is wrong.
pub fn parse(text: &str) -> Result<Vec<Key>, String> {
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    text.split(',')
        .map(|part| {
            let mut words = part.split_whitespace();
            let Some(first) = words.next() else { return Err("a sort key is missing between the commas".to_string()) };
            let field = Field::of_word(first).ok_or_else(|| format!("{first:?} is not a sort field: created, modified, due, priority, title or dependencies"))?;
            let desc = match words.next() {
                None => false,
                Some(w) if w.eq_ignore_ascii_case("asc") => false,
                Some(w) if w.eq_ignore_ascii_case("desc") => true,
                Some(w) => return Err(format!("{w:?} is not a direction: asc or desc")),
            };
            if words.next().is_some() {
                return Err(format!("{} takes one direction at most", field.word()));
            }
            if !field.directed() && part.split_whitespace().count() > 1 {
                return Err("dependencies has no direction".to_string());
            }
            Ok(Key::new(field, desc))
        })
        .collect()
}

/// The value of the `sort:` line for these keys.
pub fn write(keys: &[Key]) -> String {
    keys.iter().map(|k| if k.field.directed() { format!("{} {}", k.field.word(), if k.desc { "desc" } else { "asc" }) } else { k.field.word().to_string() }).collect::<Vec<_>>().join(", ")
}

/// The keys of a sort as the form shows them, made again when the text it was made for changes.
#[derive(Default)]
pub struct State {
    pub keys: Vec<Key>,
    /// what is wrong with the text: the keys stay as they were
    pub error: Option<String>,
    synced: Option<String>,
}

impl State {
    /// Read `text` when it is not the one the keys were made for.
    pub fn sync(&mut self, text: &str) {
        if self.synced.as_deref() == Some(text) {
            return;
        }
        match parse(text) {
            Ok(keys) => (self.keys, self.error) = (keys, None),
            Err(e) => self.error = Some(e),
        }
        self.synced = Some(text.to_string());
    }

    /// The keys were edited: the text they make, which is the one they are made for now.
    pub fn written(&mut self) -> String {
        let text = write(&self.keys);
        self.synced = Some(text.clone());
        text
    }

    /// Move the key at `from` to the place `to` (the other keys keep their order).
    pub fn move_key(&mut self, from: usize, to: usize) {
        if from < self.keys.len() && to < self.keys.len() && from != to {
            let key = self.keys.remove(from);
            self.keys.insert(to, key);
        }
    }

    /// The fields that no key uses yet.
    pub fn unused(&self) -> Vec<Field> {
        Field::ALL.into_iter().filter(|f| self.keys.iter().all(|k| k.field != *f)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    // spec: QUERY-023
    #[test]
    fn query_023_a_sort_line_is_read_and_written_in_one_way() {
        let keys = parse("Due ASC,  priority desc , dependencies").unwrap();
        assert_eq!(keys, [Key::new(Field::Due, false), Key::new(Field::Priority, true), Key::new(Field::Dependencies, false)]);
        assert_eq!(write(&keys), "due asc, priority desc, dependencies");
        assert_eq!(parse("title").unwrap(), [Key::new(Field::Title, false)], "asc is the default");
        assert_eq!(parse("  ").unwrap(), [], "blank is no key");
        assert_eq!(write(&[]), "");
    }

    // spec: QUERY-023
    #[test]
    fn query_023_a_bad_sort_line_says_what_is_wrong() {
        for (text, part) in [("colour", "not a sort field"), ("due up", "not a direction"), ("due asc desc", "one direction"), ("dependencies desc", "no direction"), ("due,,title", "missing"), ("due,", "missing")] {
            let e = parse(text).unwrap_err();
            assert!(e.contains(part), "{text}: {e}");
        }
    }

    // spec: QUERY-023
    #[test]
    fn query_023_keys_move_and_the_unused_fields_are_the_ones_to_add() {
        let mut s = State { keys: parse("due, title, created desc").unwrap(), ..Default::default() };
        s.move_key(2, 0);
        assert_eq!(write(&s.keys), "created desc, due asc, title asc");
        s.move_key(0, 9);
        s.move_key(9, 0);
        assert_eq!(write(&s.keys), "created desc, due asc, title asc", "out of range moves nothing");
        assert_eq!(s.unused(), [Field::Modified, Field::Priority, Field::Dependencies]);
        assert_eq!(s.written(), "created desc, due asc, title asc");
        s.sync("title desc");
        assert_eq!(write(&s.keys), "title desc");
        s.sync("nonsense");
        assert!(s.error.is_some(), "a text that is not read keeps the keys and says why");
        assert_eq!(write(&s.keys), "title desc");
    }

    proptest! {
        #![proptest_config(crate::property_test_support::config())]
        // spec: QUERY-023
        #[test]
        fn query_023_keys_written_are_read_back(keys in prop::collection::vec((0usize..6, any::<bool>()), 0..6)) {
            let keys: Vec<Key> = keys.into_iter().map(|(f, d)| Key::new(Field::ALL[f], d)).collect();
            prop_assert_eq!(parse(&write(&keys)).unwrap(), keys);
        }
    }
}
