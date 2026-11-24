//! Completion of citation keys and of labels while typing (EDIT-124, ADR-0039 §1 to §4, #646):
//! after `[@`, `; @` or `@` the keys of the bibliography in effect, after `@fig:`, `@tbl:`, `@eq:`
//! and `@sec:` the labels of the document. The list, its keys and its popup are those of the tag
//! completion (EDIT-120, [`crate::tag_assist::ListAssist`]); this is the source of its entries.

use crate::bibliography::format;
use crate::markdown;
use crate::markdown::citations::{self, Kind};
use crate::tag_assist::{Candidate, Source};
use crate::thesis::View;
use eframe::egui::Color32;

/// How many entries the list shows at most.
const MAX_ITEMS: usize = 8;

/// What the `@` that is being typed starts.
#[derive(Debug, PartialEq, Eq)]
pub struct CiteQuery {
    /// where the key (after the `@`) starts
    pub start: usize,
    /// what is typed of it
    pub typed: String,
    /// the `@` stands in a bracket (`[@`, `; @`): the group of a citation, which is a citation or a reference with or without a bibliography
    pub bracketed: bool,
}

fn is_key_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | ':' | '.' | '-' | '+' | '/')
}

/// The key being typed at byte `cursor` of `text`: after an `@` that starts a citation or a
/// reference by the rules of ADR-0039 §4: at the start of a line or after a space, `(`, `[`,
/// `;`, `,` or an opening quote (and `-@` after `[`), not in front matter, code, math, a link, a
/// `[[wiki link]]` or a note of mail or a feed. `None` anywhere else.
pub fn cite_query(text: &str, cursor: usize) -> Option<CiteQuery> {
    let before = text.get(..cursor)?;
    let line_start = before.rfind('\n').map_or(0, |p| p + 1);
    let line = &before[line_start..];
    let typed_len: usize = line.chars().rev().take_while(|c| is_key_char(*c)).map(char::len_utf8).sum();
    let start = cursor - typed_len;
    let at = start.checked_sub(1).filter(|&a| a >= line_start && text.as_bytes()[a] == b'@')?;
    // the word boundary (rule 2)
    let head = &text[line_start..at];
    let boundary = match head.chars().next_back() {
        None => true,
        Some(c) if c.is_whitespace() || matches!(c, '(' | '[' | ';' | ',' | '"' | '\'' | '“' | '‘' | '«') => true,
        Some('-') => head.ends_with("[-"),
        Some(_) => false,
    };
    if !boundary || typed_len > 120 {
        return None;
    }
    // front matter, code, math and the like (rule 1)
    if crate::tag_assist::front_matter_line(text, line_start) {
        return None;
    }
    if head.matches('`').count() % 2 == 1 || head.rfind("[[").is_some_and(|o| head[o..].find("]]").is_none()) || head.rfind("](").is_some_and(|o| head[o..].find(')').is_none()) {
        return None;
    }
    if markdown::fenced_code_lines(&text[..cursor]).last().copied().unwrap_or(false) {
        return None;
    }
    // inside `$…$` on the line, or in a display formula that is open (an odd number of `$$` before)
    if text[..cursor].matches("$$").count() % 2 == 1 || head.replace("$$", "").matches('$').count() % 2 == 1 {
        return None;
    }
    if markdown::front_matter_values(text).iter().any(|(k, v)| k == "type" && v.first().is_some_and(|t| matches!(t.trim().to_lowercase().as_str(), "email" | "social-post"))) {
        return None;
    }
    Some(CiteQuery { start, typed: text[start..cursor].to_string(), bracketed: in_bracket(head) })
}

/// Whether the text of a line before an `@` is inside a bracket that is not closed.
fn in_bracket(head: &str) -> bool {
    head.rfind('[').is_some_and(|o| head[o..].find(']').is_none())
}

/// The kind of label a key being typed asks for (`fig:re`: figures), if it has typed the prefix.
fn label_kind(typed: &str) -> Option<Kind> {
    [Kind::Figure, Kind::Table, Kind::Equation, Kind::Section].into_iter().find(|k| typed.starts_with(&format!("{}:", k.prefix())))
}

/// The keys and the labels of a document as the entries of the list.
pub(crate) struct Cites<'a>(pub &'a View);

impl Source for Cites<'_> {
    fn area(&self) -> &'static str {
        "cites"
    }

    fn query(&self, text: &str, cursor: usize) -> Option<(usize, String)> {
        let q = cite_query(text, cursor)?;
        // a bare `@key` is a citation only while a bibliography holds keys (rule 4); a reference always is one
        (q.bracketed || label_kind(&q.typed).is_some() || self.0.has_bibliography()).then_some((q.start, q.typed))
    }

    fn load(&self) -> Vec<Candidate> {
        let keys = self.0.bib.entries().map(|e| Candidate { insert: e.key.clone(), label: e.key.clone(), detail: format::summary(e) });
        let labels = self.0.labels.entries.iter().filter(|e| e.kind.is_labelled()).map(|e| {
            let id = format!("{}:{}", e.kind.prefix(), e.id);
            let caption = if e.caption.is_empty() { String::new() } else { format!(" · {}", e.caption) };
            Candidate { insert: id.clone(), label: id, detail: format!("{}{caption}", crate::thesis::reference(e, self.0.lang)) }
        });
        keys.chain(labels).collect()
    }

    fn rank(&self, query: &str, all: &[Candidate]) -> Vec<Candidate> {
        let prefix = label_kind(query);
        let mut hits: Vec<(i32, &Candidate)> = all
            .iter()
            .filter(|c| citations::reserved_prefix(&c.insert).is_some() == prefix.is_some())
            .filter(|c| prefix.is_none_or(|k| citations::reserved_prefix(&c.insert) == Some(k)))
            .filter(|c| c.insert != query)
            .filter_map(|c| {
                let by_key = markdown::fuzzy_score(query, &c.insert);
                // what is typed may be a word of the authors or the title as well
                let by_text = (!query.is_empty() && c.detail.to_lowercase().contains(&query.to_lowercase())).then_some(50);
                by_key.max(by_text).map(|s| (s, c))
            })
            .collect();
        hits.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.insert.cmp(&b.1.insert)));
        hits.into_iter().take(MAX_ITEMS).map(|(_, c)| c.clone()).collect()
    }

    fn colour(&self) -> Color32 {
        crate::theme::palette().link
    }

    fn notice(&self, text: &str, start: usize, typed: &str) -> Option<String> {
        // a bracket with a key being typed, and no bibliography to list from: say so (EDIT-124)
        let head = &text[text[..start].rfind('\n').map_or(0, |p| p + 1)..start.saturating_sub(1)];
        (in_bracket(head) && label_kind(typed).is_none() && !self.0.has_bibliography()).then(|| if self.0.bib.loading { "Reading the bibliography…".to_string() } else { "No bibliography is in effect for this note.".to_string() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(text: &str) -> Option<(usize, String, bool)> {
        cite_query(text, text.len()).map(|q| (q.start, q.typed, q.bracketed))
    }

    // spec: EDIT-124
    #[test]
    fn edit_124_after_a_bracket_a_semicolon_or_a_space_an_at_opens_a_query() {
        assert_eq!(q("see [@kn"), Some((6, "kn".into(), true)));
        assert_eq!(q("[@"), Some((2, String::new(), true)));
        assert_eq!(q("[see @kn"), Some((6, "kn".into(), true)));
        assert_eq!(q("[@a; @kn"), Some((6, "kn".into(), true)));
        assert_eq!(q("[@a;@kn"), Some((5, "kn".into(), true)));
        assert_eq!(q("[-@kn"), Some((3, "kn".into(), true)));
        assert_eq!(q("as @kn"), Some((4, "kn".into(), false)));
        assert_eq!(q("@kn"), Some((1, "kn".into(), false)));
        assert_eq!(q("(@kn"), Some((2, "kn".into(), false)));
        assert_eq!(q("[@fig:re"), Some((2, "fig:re".into(), true)));
        assert_eq!(q("@chen:2019.de"), Some((1, "chen:2019.de".into(), false)));
        assert_eq!(cite_query("a [@kn more", 6).map(|q| q.typed), Some("kn".into()), "the cursor decides, not the end");
    }

    // spec: EDIT-124
    #[test]
    fn edit_124_an_at_in_a_word_an_address_a_url_or_after_a_dash_is_no_query() {
        assert_eq!(q("ada@host"), None);
        assert_eq!(q("mail ada@host.org"), None);
        assert_eq!(q("see https://example.com/@user"), None);
        assert_eq!(q("a-@kn"), None);
        assert_eq!(q("a.@kn"), None);
        assert_eq!(q("\\@kn"), None, "an escaped @ is a plain @");
    }

    // spec: EDIT-124
    #[test]
    fn edit_124_front_matter_code_math_links_and_foreign_notes_are_no_query() {
        assert_eq!(q("---\nauthor: @kn"), None);
        assert_eq!(q("```\n@kn"), None);
        assert_eq!(q("use `@kn"), None);
        assert_eq!(q("$a @kn"), None);
        assert_eq!(q("$$\n@kn"), None);
        assert_eq!(q("[[Note @kn"), None);
        assert_eq!(q("[a](http://x/@kn"), None);
        assert_eq!(q("---\ntype: email\n---\n[@kn"), None);
        assert_eq!(q("---\ntype: social-post\n---\n[@kn"), None);
        assert_eq!(q("---\ntitle: T\n---\n[@kn").map(|q| q.1), Some("kn".into()), "after the front matter it is the body");
        assert_eq!(q("```\ncode\n```\n[@kn").map(|q| q.1), Some("kn".into()));
    }
}
