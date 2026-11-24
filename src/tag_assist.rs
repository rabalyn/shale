//! Tag completion while typing (EDIT-120): `#ta` in the text, or a word in a `tags:` property of
//! the front matter, opens a list of the vault's tags that match, best match first. It is a part of
//! [`crate::link_assist::LinkAssist`], which calls [`ListAssist::take_keys`] before the text box and
//! [`ListAssist::after`] after it, like for the `[[` list.

use crate::colour_tags;
use crate::link_assist::{down_key, up_key};
use crate::markdown;
use crate::theme;
use crate::vault::Vault;
use eframe::egui::{self, Key, Modifiers, Ui, Vec2};
use egui::text::{CCursor, CCursorRange};

/// How many tags the list shows at most.
const MAX_ITEMS: usize = 8;

/// A character a tag is made of (as `markdown::find_tags` reads them).
fn is_tag_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | '/')
}

/// The tag being typed at byte `cursor` of `text`: (byte where its name starts, the name typed so far,
/// without the `#`). `None` when the cursor is not right behind a tag of at least one character that
/// is not only digits, or when that place takes no tag: fenced code, inline code, a link `[[…`, the
/// colour of a `<mark>` or a `#` inside a word (a URL, `a#b`), and a heading (`# ` has a space).
/// In the front matter only the value of a `tags:` property takes one, with or without the `#`.
pub fn tag_query(text: &str, cursor: usize) -> Option<(usize, String)> {
    let before = text.get(..cursor)?;
    let line_start = before.rfind('\n').map_or(0, |p| p + 1);
    let line = &before[line_start..];
    let typed_len: usize = line.chars().rev().take_while(|c| is_tag_char(*c)).map(char::len_utf8).sum();
    if typed_len == 0 || typed_len > 80 {
        return None;
    }
    let start = cursor - typed_len;
    let name = &text[start..cursor];
    if name.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if front_matter_line(text, line_start) {
        return tags_value_before(text, line_start, start).then(|| (start, name.to_string()));
    }
    // the body: a `#` right before the name, itself after a space, a tab, a `(` or at the line start
    let hash = start.checked_sub(1).filter(|&h| text.as_bytes()[h] == b'#')?;
    let at = hash - line_start;
    if at > 0 && !matches!(line.as_bytes()[at - 1], b' ' | b'\t' | b'(') {
        return None;
    }
    let before_hash = &line[..at];
    // inside a `code` span (an odd number of backticks before), or an unclosed `[[` link
    if before_hash.matches('`').count() % 2 == 1 {
        return None;
    }
    if before_hash.rfind("[[").is_some_and(|o| before_hash[o..].find("]]").is_none()) {
        return None;
    }
    let line_end = text[cursor..].find('\n').map_or(text.len(), |p| cursor + p);
    if colour_tags::open_tags(&text[line_start..line_end]).iter().any(|r| r.contains(&at)) {
        return None;
    }
    // fenced code: the lines up to this one
    if markdown::fenced_code_lines(&text[..cursor]).last().copied().unwrap_or(false) {
        return None;
    }
    Some((start, name.to_string()))
}

/// Is the line starting at byte `line_start` a line of the properties, between the two `---`?
pub(crate) fn front_matter_line(text: &str, line_start: usize) -> bool {
    if line_start == 0 || !(text.starts_with("---\n") || text.starts_with("---\r\n")) {
        return false;
    }
    for (off, l) in markdown::lines_with_offsets(text).into_iter().skip(1) {
        if off >= line_start {
            return true;
        }
        if l.trim() == "---" {
            return false;
        }
    }
    false
}

/// Is the name starting at `start` in the line at `line_start` a value of the `tags:` property: after
/// `tags:`, `[`, `,`, a space or a `#` on that line, or an item `- name` of a list that `tags:` opens?
fn tags_value_before(text: &str, line_start: usize, start: usize) -> bool {
    let line = text[line_start..start].trim_end_matches(['#', '"', '\'']);
    if let Some(rest) = line.strip_prefix("tags:").or_else(|| line.strip_prefix("tag:")) {
        return rest.chars().all(|c| matches!(c, ' ' | '\t' | '[' | ',' | '"' | '\'' | '#') || is_tag_char(c)) && (rest.is_empty() || rest.ends_with([' ', '[', ',', '\t']));
    }
    if line.trim_start().strip_prefix('-').is_some_and(|r| r.trim().is_empty()) {
        // the property that owns the list: the nearest line above that is no list item
        let mut owner = text[..line_start].lines().rev().skip_while(|l| l.trim_start().starts_with("- "));
        return owner.next().is_some_and(|l| matches!(l.trim_end(), "tags:" | "tag:"));
    }
    false
}

/// The tags that match `query`, the best match first: a prefix before a match inside a name before a
/// match of the letters in order (`prj` finds `projects/shale`), then the shorter, then alphabetical
/// (`markdown::fuzzy_score`, as the `[[` list ranks notes). The tag that is exactly the query is left
/// out: there is nothing to complete.
pub fn rank(query: &str, tags: &[String]) -> Vec<String> {
    let mut hits: Vec<(i32, &String)> = tags.iter().filter(|t| t.as_str() != query).filter_map(|t| markdown::fuzzy_score(query, t).map(|s| (s, t))).collect();
    hits.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(b.1)));
    hits.into_iter().take(MAX_ITEMS).map(|(_, t)| t.clone()).collect()
}

/// One line of a completion list: what is typed in when it is picked, how it is shown, and what is said beside it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Candidate {
    pub insert: String,
    pub label: String,
    pub detail: String,
}

/// What a completion list is a list of: tags (EDIT-120), or the keys and labels of a thesis (EDIT-124).
/// [`ListAssist`] does the keys, the popup and the pick for all of them.
pub(crate) trait Source {
    /// the id of the popup, so that two lists of one text box are two areas
    fn area(&self) -> &'static str;
    /// the name being typed at `cursor`: where it starts, and what is typed
    fn query(&self, text: &str, cursor: usize) -> Option<(usize, String)>;
    /// every candidate, read once when the list opens (not on every key)
    fn load(&self) -> Vec<Candidate>;
    /// the candidates that match `query`, the best first
    fn rank(&self, query: &str, all: &[Candidate]) -> Vec<Candidate>;
    fn colour(&self) -> egui::Color32;
    /// what to say instead of a list when nothing can be listed and the place wants an answer
    fn notice(&self, _text: &str, _start: usize, _typed: &str) -> Option<String> {
        None
    }
}

/// The tags of the vault (EDIT-120).
pub(crate) struct Tags<'a>(pub &'a Vault);

impl Source for Tags<'_> {
    fn area(&self) -> &'static str {
        "tags"
    }

    fn query(&self, text: &str, cursor: usize) -> Option<(usize, String)> {
        tag_query(text, cursor)
    }

    fn load(&self) -> Vec<Candidate> {
        self.0.all_tags().into_iter().map(|(t, _)| Candidate { insert: t.clone(), label: format!("#{t}"), detail: String::new() }).collect()
    }

    fn rank(&self, query: &str, all: &[Candidate]) -> Vec<Candidate> {
        let names: Vec<String> = all.iter().map(|c| c.insert.clone()).collect();
        rank(query, &names).into_iter().filter_map(|t| all.iter().find(|c| c.insert == t).cloned()).collect()
    }

    fn colour(&self) -> egui::Color32 {
        theme::palette().tag
    }
}

/// State of a completion list of one text box.
#[derive(Default)]
pub(crate) struct ListAssist {
    pub(crate) active: bool,
    pub(crate) sel: usize,
    pub(crate) items: Vec<Candidate>,
    /// what the list was read for (read once when it opens)
    all: Vec<Candidate>,
    /// byte where the name being typed starts
    start: Option<usize>,
    /// Esc was pressed for the name starting at this byte
    dismissed: Option<usize>,
    /// the name the items were ranked for
    ranked_for: Option<String>,
    /// this frame: the entry picked by the keyboard
    pub(crate) pick: Option<usize>,
    /// a sentence shown instead of a list (no bibliography): there is nothing to pick
    pub(crate) notice: Option<String>,
    /// this frame: the link of the notice was clicked
    pub(crate) notice_clicked: bool,
}

impl ListAssist {
    /// Before the text box: ↑↓ (Ctrl+J/K) choose, Enter or Tab pick, Esc closes.
    pub(crate) fn take_keys(&mut self, ui: &mut Ui) {
        self.pick = None;
        self.notice_clicked = false;
        if self.notice.is_some() {
            ui.input_mut(|i| {
                if i.consume_key(Modifiers::NONE, Key::Escape) {
                    self.dismissed = self.start;
                    self.notice = None;
                }
            });
            return;
        }
        if !self.active || self.items.is_empty() {
            return;
        }
        let n = self.items.len();
        ui.input_mut(|i| {
            if down_key(i) {
                self.sel = (self.sel + 1) % n;
            }
            if up_key(i) {
                self.sel = (self.sel + n - 1) % n;
            }
            if i.consume_key(Modifiers::NONE, Key::Enter) || i.consume_key(Modifiers::NONE, Key::Tab) {
                self.pick = Some(self.sel.min(n - 1));
            }
            if i.consume_key(Modifiers::NONE, Key::Escape) {
                self.active = false;
                self.dismissed = self.start;
            }
        });
    }

    /// Whether the list or its notice is on the screen.
    pub(crate) fn is_open(&self) -> bool {
        self.active || self.notice.is_some()
    }

    /// After the text box (`id` is its id): open, filter and draw the list, and complete the pick.
    /// True when the text changed.
    pub(crate) fn after(&mut self, ui: &mut Ui, id: egui::Id, text: &mut String, source: &dyn Source, output: &egui::text_edit::TextEditOutput, typing: bool) -> bool {
        let resp = &output.response.response;
        let area = id.with(source.area());
        let shown = self.active || self.notice.is_some();
        let on_list = shown && ui.input(|i| i.pointer.interact_pos()).zip(ui.ctx().memory(|m| m.area_rect(area))).is_some_and(|(p, r)| r.contains(p));
        if on_list && !resp.has_focus() {
            ui.ctx().memory_mut(|m| m.request_focus(id));
            ui.ctx().request_repaint();
        }
        let stored = || egui::text_edit::TextEditState::load(ui.ctx(), id).and_then(|st| st.cursor.char_range()).filter(|r| r.is_empty()).map(|r| r.primary.index.0);
        let cursor = output.cursor_range.filter(|r| r.is_empty()).map(|r| r.primary.index.0).or_else(|| if on_list { stored() } else { None });
        let query = if (resp.has_focus() || on_list) && typing { cursor.and_then(|c| source.query(text, markdown::char_to_byte(text, c))) } else { None };
        // a dismissed name stays closed until the cursor leaves it
        if query.is_none() {
            self.dismissed = None;
        }
        let query = query.filter(|(s, _)| Some(*s) != self.dismissed);
        self.notice = None;
        let Some((start, name)) = query else {
            self.active = false;
            self.start = None;
            self.ranked_for = None;
            return false;
        };
        if !self.active {
            // the candidates are read once when the list opens, not on every key
            self.all = source.load();
            self.sel = 0;
            self.ranked_for = None;
        }
        self.start = Some(start);
        if self.ranked_for.as_deref() != Some(name.as_str()) {
            self.items = source.rank(&name, &self.all);
            self.ranked_for = Some(name);
            self.sel = 0;
        }
        self.active = !self.items.is_empty();
        let below = |c: usize| output.galley_pos + output.galley.pos_from_cursor(CCursor::new(c)).left_bottom().to_vec2() + Vec2::new(0.0, 4.0);
        if !self.active {
            // nothing to list: a place that wants an answer says why
            if let Some(why) = source.notice(text, start, self.ranked_for.as_deref().unwrap_or_default()) {
                self.notice_clicked = Self::notice_popup(ui, area, below(cursor.unwrap_or(0)), &why);
                self.notice = Some(why);
            }
            return false;
        }
        let picked = match self.pick.take() {
            Some(p) => Some(p),
            None => {
                let clicked = self.popup(ui, area, below(cursor.unwrap_or(0)), source.colour());
                if clicked.is_some() {
                    ui.ctx().memory_mut(|m| m.request_focus(id));
                }
                clicked
            }
        };
        let Some(item) = picked.and_then(|i| self.items.get(i).cloned()) else { return false };
        let cur = markdown::char_to_byte(text, cursor.unwrap_or(0));
        text.replace_range(start..cur, &item.insert);
        let c = markdown::byte_to_char(text, start + item.insert.len());
        let mut state = egui::text_edit::TextEditState::load(ui.ctx(), id).unwrap_or_default();
        state.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(c))));
        state.store(ui.ctx(), id);
        self.active = false;
        self.ranked_for = None;
        true
    }

    /// Draw the list at `pos`; returns the entry clicked.
    fn popup(&self, ui: &Ui, area: egui::Id, pos: egui::Pos2, colour: egui::Color32) -> Option<usize> {
        let mut clicked = None;
        egui::Area::new(area).order(egui::Order::Foreground).fixed_pos(pos).show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_min_width(200.0);
                for (i, c) in self.items.iter().enumerate() {
                    let mut job = egui::text::LayoutJob::default();
                    let font = egui::TextStyle::Body.resolve(ui.style());
                    job.append(&c.label, 0.0, egui::TextFormat { font_id: font.clone(), color: colour, ..Default::default() });
                    if !c.detail.is_empty() {
                        job.append(&format!("  {}", c.detail), 0.0, egui::TextFormat { font_id: egui::FontId::new(font.size * 0.85, font.family.clone()), color: theme::palette().muted, ..Default::default() });
                    }
                    // a long entry is cut to the width of the list, not stretched over the page
                    job.wrap = egui::text::TextWrapping { max_width: 480.0, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
                    if ui.selectable_label(i == self.sel, job).clicked() {
                        clicked = Some(i);
                    }
                }
                ui.label(egui::RichText::new("↑↓ or Ctrl+J/K to move · Enter, Tab or click to pick · Esc to close").small().color(theme::palette().faint));
            });
        });
        clicked
    }

    /// Draw the sentence that says why there is no list; true when its link was clicked.
    fn notice_popup(ui: &Ui, area: egui::Id, pos: egui::Pos2, why: &str) -> bool {
        let mut clicked = false;
        egui::Area::new(area).order(egui::Order::Foreground).fixed_pos(pos).show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.label(egui::RichText::new(why).color(theme::palette().muted));
                clicked = ui.link("Choose a bibliography file in Settings").clicked();
            });
        });
        clicked
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(text: &str) -> Option<(usize, String)> {
        tag_query(text, text.len())
    }

    fn tags(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    // spec: EDIT-120
    #[test]
    fn edit_120_a_hash_and_a_letter_after_a_space_or_at_the_line_start_open_a_query() {
        assert_eq!(q("#pr"), Some((1, "pr".into())));
        assert_eq!(q("see #pro/sh"), Some((5, "pro/sh".into())));
        assert_eq!(q("(#pr"), Some((2, "pr".into())));
        assert_eq!(q("line\n\t#pr"), Some((7, "pr".into())));
        assert_eq!(tag_query("a #pro more", 5), Some((3, "pr".into())), "the cursor decides, not the end of the text");
    }

    // spec: EDIT-120
    #[test]
    fn edit_120_a_heading_a_bare_hash_digits_and_a_hash_in_a_word_are_no_query() {
        assert_eq!(q("# "), None);
        assert_eq!(q("# Title"), None);
        assert_eq!(q("## Title"), None);
        assert_eq!(q("#"), None);
        assert_eq!(q("see #"), None);
        assert_eq!(q("issue #12"), None);
        assert_eq!(q("a#b"), None);
        assert_eq!(q("https://example.com/page#frag"), None);
        assert_eq!(q("[x](https://example.com/#frag"), None);
        assert_eq!(q("[[Note#Head"), None);
        assert_eq!(q("[[a #b"), None);
        assert_eq!(q("see [[a]] #b"), Some((11, "b".into())), "a closed link does not block");
    }

    // spec: EDIT-120
    #[test]
    fn edit_120_code_blocks_inline_code_and_colours_are_no_query() {
        assert_eq!(q("```\n#pro"), None);
        assert_eq!(q("~~~rust\nlet a = 1;\n#pro"), None);
        assert_eq!(q("```\ncode\n```\n#pro"), Some((14, "pro".into())), "after the closing fence it is text again");
        assert_eq!(q("use `a #pro"), None);
        assert_eq!(q("`a` and #pro"), Some((9, "pro".into())));
        let mark = r#"<mark style="background: #FFFF00;">x</mark>"#;
        assert_eq!(tag_query(mark, mark.find(';').unwrap()), None, "the colour of a mark is no tag");
    }

    // spec: EDIT-120
    #[test]
    fn edit_120_the_value_of_a_tags_property_takes_a_name_with_or_without_the_hash() {
        assert_eq!(q("---\ntags: pro"), Some((10, "pro".into())));
        assert_eq!(q("---\ntags: [a, pro"), Some((14, "pro".into())));
        assert_eq!(q("---\ntags: [a,pro"), Some((13, "pro".into())));
        assert_eq!(q("---\ntags: #pro"), Some((11, "pro".into())));
        assert_eq!(q("---\ntags:\n  - pro"), Some((14, "pro".into())));
        assert_eq!(q("---\ntags:\n  - a\n  - pro"), Some((20, "pro".into())));
        assert_eq!(q("---\ntitle: pro"), None, "another property");
        assert_eq!(q("---\ntags: [a]\ntitle: pro"), None);
        assert_eq!(q("---\ntags:\n  - a\ntitle:\n  - pro"), None, "a list of another property");
        assert_eq!(q("---\ntags: a\n---\n#pro"), Some((17, "pro".into())), "after the front matter it is the body");
    }

    // spec: EDIT-120
    #[test]
    fn edit_120_the_matches_are_ranked_prefix_before_inside_before_letters_in_order() {
        let all = tags(&["projects/shale", "my-projects", "prj-x", "alpha", "project", "pxrxj"]);
        assert_eq!(rank("proj", &all), tags(&["project", "projects/shale", "my-projects"]), "prefix (shorter first), then inside a name");
        assert_eq!(rank("prj", &all), tags(&["prj-x", "pxrxj", "project", "my-projects", "projects/shale"]), "a prefix before the letters in order, then the fewer letters skipped");
    }

    // spec: EDIT-120
    #[test]
    fn edit_120_a_fuzzy_query_finds_a_nested_tag_and_an_exact_tag_is_not_offered() {
        let all = tags(&["projects/shale", "projects", "ideas/prj"]);
        assert_eq!(rank("prj", &all), tags(&["ideas/prj", "projects", "projects/shale"]), "inside a name, then letters in order");
        assert_eq!(rank("projects/s", &all), tags(&["projects/shale"]));
        assert_eq!(rank("projects", &all), tags(&["projects/shale"]), "the tag typed in full is not offered again");
        assert!(rank("zzz", &all).is_empty());
        assert_eq!(rank("PRO", &all)[0], "projects", "case does not matter");
    }
}
