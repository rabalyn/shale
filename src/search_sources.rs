//! Which sources the search over everything looks at (NAV-050…NAV-056, #478): notes, mail and
//! posts. The pieces both views of the search (the `Space f g` picker and the sidebar) share:
//!
//! - [`Kind`] and [`Selection`]: a source and a non-empty set of them;
//! - [`Prefixes`]: the letters of `f:`, `m:`, `s:` and `fm:` at the start of a query, parsed in
//!   the engine (`search_all::Query`) and set in Settings → Search ([`SearchConfig`]);
//! - [`Filter`]: the solo of the count buttons, which is not saved;
//! - [`SearchConfig`]: the letters and the default selection, which are (global settings);
//! - [`bar`]: the row of count buttons in the header of both views.

use crate::search_all::query::{Cache, QueryError};
use crate::search_all::{MAIL, NOTES, POSTS};
use eframe::egui::{self, RichText, Ui};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// A source of the search.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Notes,
    Mail,
    Posts,
}

impl Kind {
    /// The sources in the order they are listed and counted.
    pub const ALL: [Kind; 3] = [Kind::Notes, Kind::Mail, Kind::Posts];

    /// The name of the group of this source's hits (and of its button).
    pub fn group(self) -> &'static str {
        match self {
            Kind::Notes => NOTES,
            Kind::Mail => MAIL,
            Kind::Posts => POSTS,
        }
    }

    /// The word the settings file uses.
    fn id(self) -> &'static str {
        match self {
            Kind::Notes => "notes",
            Kind::Mail => "mail",
            Kind::Posts => "posts",
        }
    }

    /// The source a group of hits belongs to (none for the groups of other lists).
    pub fn of_group(group: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.group() == group)
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// A set of sources. It is never empty when it is made with [`Selection::from_ids`] or
/// [`Selection::toggled`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Selection(u8);

impl Selection {
    /// Notes, mail and posts.
    pub const ALL: Selection = Selection(0b111);

    pub fn only(kind: Kind) -> Selection {
        Selection(1 << kind.index())
    }

    pub fn contains(self, kind: Kind) -> bool {
        self.0 & (1 << kind.index()) != 0
    }

    fn with(self, kind: Kind) -> Selection {
        Selection(self.0 | 1 << kind.index())
    }

    /// The sources in the order of [`Kind::ALL`].
    pub fn kinds(self) -> impl Iterator<Item = Kind> {
        Kind::ALL.into_iter().filter(move |k| self.contains(*k))
    }

    /// The selection with `kind` switched in or out; none when that would leave nothing (the
    /// last selected source cannot be switched off, NAV-053).
    pub fn toggled(self, kind: Kind) -> Option<Selection> {
        let toggled = Selection(self.0 ^ (1 << kind.index()));
        (toggled.0 != 0).then_some(toggled)
    }

    /// The selection a list of words of the settings file stands for; words that are not a source
    /// are left out, and a list with none is every source (NAV-054).
    pub fn from_ids(ids: &[String]) -> Selection {
        let sel = ids.iter().filter_map(|id| Kind::ALL.into_iter().find(|k| k.id() == id.trim().to_lowercase())).fold(Selection(0), Selection::with);
        if sel.0 == 0 {
            Selection::ALL
        } else {
            sel
        }
    }

    fn ids(self) -> Vec<String> {
        self.kinds().map(|k| k.id().to_string()).collect()
    }
}

/// The letters that start a query with the sources it is for: `f:` notes, `m:` mail, `s:` posts
/// (NAV-050).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Prefixes {
    pub notes: char,
    pub mail: char,
    pub posts: char,
}

impl Default for Prefixes {
    fn default() -> Self {
        Prefixes { notes: 'f', mail: 'm', posts: 's' }
    }
}

/// A query cut into the sources its prefix names and the text to look for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Parsed<'a> {
    /// the sources of the prefix; none: the query has no prefix
    pub prefix: Option<Selection>,
    pub text: &'a str,
}

impl Prefixes {
    fn kind_of(&self, c: char) -> Option<Kind> {
        let c = c.to_lowercase().next().unwrap_or(c);
        [(self.notes, Kind::Notes), (self.mail, Kind::Mail), (self.posts, Kind::Posts)].into_iter().find(|(l, _)| *l == c).map(|(_, k)| k)
    }

    /// Cut a prefix off the start of `query` (NAV-050): the letters of the sources, each at most
    /// once, then a colon, and a space after the colon or not. A word before the colon that is
    /// made of anything else is no prefix: `from:ann` is text (a field of mail and posts).
    pub fn parse<'a>(&self, query: &'a str) -> Parsed<'a> {
        let none = Parsed { prefix: None, text: query };
        let Some((word, rest)) = query.trim_start().split_once(':') else { return none };
        let mut sel = Selection(0);
        for c in word.chars() {
            match self.kind_of(c) {
                Some(k) if !sel.contains(k) => sel = sel.with(k),
                _ => return none,
            }
        }
        if sel.0 == 0 {
            return none;
        }
        Parsed { prefix: Some(sel), text: rest.trim_start() }
    }
}

/// The words that limit a term of a mail or post search to a field (MAIL-012, SOC-031).
fn field_words() -> impl Iterator<Item = &'static str> {
    crate::mail::db::FIELDS.iter().map(|(w, _)| *w).chain(crate::social::post_index::FIELDS.iter().map(|(w, _)| *w))
}

/// The prefixes the three letters make, or why they cannot be used (NAV-051): each is one letter,
/// no two are the same, and no field word of mail or posts (`from`, `to`, `in`, …) can be spelled
/// with them, because `fm:` is a prefix too and must not be `from:` in disguise.
pub fn check_prefixes(notes: &str, mail: &str, posts: &str) -> Result<Prefixes, String> {
    let letter = |what: &str, s: &str| -> Result<char, String> {
        let mut it = s.trim().chars();
        match (it.next(), it.next()) {
            (Some(c), None) if c.is_alphabetic() => Ok(c.to_lowercase().next().unwrap_or(c)),
            _ => Err(format!("The prefix for {what} is one letter.")),
        }
    };
    let p = Prefixes { notes: letter("notes", notes)?, mail: letter("mail", mail)?, posts: letter("posts", posts)? };
    for (a, b, wa, wb) in [(p.notes, p.mail, "notes", "mail"), (p.notes, p.posts, "notes", "posts"), (p.mail, p.posts, "mail", "posts")] {
        if a == b {
            return Err(format!("“{a}” is the prefix of {wa} and of {wb}."));
        }
    }
    let letters = [p.notes, p.mail, p.posts];
    for word in field_words() {
        let chars: Vec<char> = word.chars().collect();
        let spelled = chars.iter().all(|c| letters.contains(c)) && chars.iter().enumerate().all(|(i, c)| !chars[..i].contains(c));
        if spelled {
            return Err(format!("“{word}:” is a field of the mail and post search, and these letters would spell it."));
        }
    }
    Ok(p)
}

/// The settings of the search (CFG-067, a table `search` of the settings file).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(default)]
pub struct SearchConfig {
    /// The one letter of the prefix for notes (`f:`).
    pub prefix_notes: String,
    /// The one letter of the prefix for mail (`m:`).
    pub prefix_mail: String,
    /// The one letter of the prefix for posts (`s:`).
    pub prefix_posts: String,
    /// The default selection: the sources a query without prefix looks in and lists, as words
    /// (`notes`, `mail`, `posts`); empty or unknown reads as all.
    pub sources: Vec<String>,
    /// How a hit of a note is shown, as a word (`follow`, `raw`, `rendered`); an unknown word
    /// reads as `follow` (NAV-058, CFG-068).
    pub hit_display: String,
}

impl Default for SearchConfig {
    fn default() -> Self {
        SearchConfig { prefix_notes: "f".into(), prefix_mail: "m".into(), prefix_posts: "s".into(), sources: Selection::ALL.ids(), hit_display: HitDisplay::Follow.id().into() }
    }
}

/// How the lines of notes that the search found are shown (NAV-058).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum HitDisplay {
    /// like the editor shows notes now ([`HitDisplay::rendered`])
    #[default]
    Follow,
    /// the markdown as it is written
    Raw,
    /// drawn as the editor draws a line away from its cursor
    Rendered,
}

impl HitDisplay {
    pub const ALL: [HitDisplay; 3] = [HitDisplay::Follow, HitDisplay::Raw, HitDisplay::Rendered];

    /// The word the settings file uses.
    pub fn id(self) -> &'static str {
        match self {
            HitDisplay::Follow => "follow",
            HitDisplay::Raw => "raw",
            HitDisplay::Rendered => "rendered",
        }
    }

    /// The name in Settings → Search.
    pub fn label(self) -> &'static str {
        match self {
            HitDisplay::Follow => "Follow the editor",
            HitDisplay::Raw => "Raw markdown",
            HitDisplay::Rendered => "Rendered",
        }
    }

    /// The setting for a word of the settings file (case ignored); an unknown one is the default.
    pub fn from_id(id: &str) -> HitDisplay {
        HitDisplay::ALL.into_iter().find(|d| d.id().eq_ignore_ascii_case(id.trim())).unwrap_or_default()
    }

    /// Is a line drawn rendered? `raw` and `rendered` say so; `follow` is the editor's: rendered
    /// when the Markdown preview is on (`live_preview`, EDIT-016) or the note in view is in the
    /// reading view (`reading`, Ctrl+E), raw otherwise (NAV-058).
    pub fn rendered(self, live_preview: bool, reading: bool) -> bool {
        match self {
            HitDisplay::Follow => live_preview || reading,
            HitDisplay::Raw => false,
            HitDisplay::Rendered => true,
        }
    }
}

impl SearchConfig {
    /// The prefixes; the default letters when the settings file holds letters that [`check_prefixes`] refuses.
    pub fn prefixes(&self) -> Prefixes {
        check_prefixes(&self.prefix_notes, &self.prefix_mail, &self.prefix_posts).unwrap_or_default()
    }

    pub fn selection(&self) -> Selection {
        Selection::from_ids(&self.sources)
    }

    pub fn set_selection(&mut self, sel: Selection) {
        self.sources = sel.ids();
    }

    pub fn display(&self) -> HitDisplay {
        HitDisplay::from_id(&self.hit_display)
    }
}

/// What the count buttons did to the list of one view: a solo is the one source that is listed
/// (NAV-052). It is not saved; none is the default selection.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Filter {
    pub solo: Option<Kind>,
}

impl Filter {
    /// A click on the button of `kind`: it is soloed; again: back to the default selection.
    pub fn click(&mut self, kind: Kind) {
        self.solo = if self.solo == Some(kind) { None } else { Some(kind) };
    }

    /// Tab / Shift+Tab: solo the next / previous of `available` (the sources the view has), and
    /// after the last the default selection again.
    pub fn step(&mut self, forward: bool, available: &[Kind]) {
        let at = self.solo.and_then(|k| available.iter().position(|a| *a == k)).map_or(0, |i| i + 1);
        let n = available.len() + 1;
        let to = if forward { (at + 1) % n } else { (at + n - 1) % n };
        self.solo = to.checked_sub(1).map(|i| available[i]);
    }

    /// The sources listed when the query has no prefix.
    pub fn shown(self, default: Selection) -> Selection {
        self.solo.map_or(default, Selection::only)
    }
}

/// How many hits each source found (at most the view's limit each); the sources not listed are counted too.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Counts(pub [usize; 3]);

impl Counts {
    pub fn get(&self, kind: Kind) -> usize {
        self.0[kind.index()]
    }

    /// "Mail 20+": the count of a source whose hits were cut at `limit` has a plus.
    pub fn text(&self, kind: Kind, limit: usize) -> String {
        let n = self.get(kind);
        format!("{} {n}{}", kind.group(), if n >= limit { "+" } else { "" })
    }
}

/// What the row of buttons is drawn for.
pub struct Bar<'a> {
    /// the counts of the list; none while nothing is typed
    pub counts: Option<&'a Counts>,
    pub limit: usize,
    pub mail_open: bool,
    /// the sources the list shows now (the prefix, the solo or the default selection)
    pub listed: Selection,
    /// the query has a prefix: it decides, the buttons wait
    pub prefixed: bool,
    /// a query is on the worker: the list is the last valid one, a spinner says so (QUERY-018)
    pub searching: bool,
}

impl Bar<'_> {
    /// The sources that have a button: mail only while its store is open.
    pub fn available(mail_open: bool) -> Vec<Kind> {
        Kind::ALL.into_iter().filter(|k| *k != Kind::Mail || mail_open).collect()
    }
}

/// What the search says when its list is partial (QUERY-010).
pub const STOPPED_EARLY: &str = "Stopped early, narrow the query";

/// The layout of the text box of a search: the text as it is, with the bytes of what is wrong
/// with the query underlined (QUERY-008). The check comes from `cache`, so the text is parsed when
/// it changes and not in every frame (QUERY-013). `size`: the font of the box, or its text style.
pub fn error_layouter<'a>(cache: &'a mut Cache, prefixes: Prefixes, size: Option<f32>) -> impl FnMut(&Ui, &dyn egui::TextBuffer, f32) -> Arc<egui::Galley> + 'a {
    move |ui, buf, wrap_width| {
        let text = buf.as_str();
        let span = cache.get(text, prefixes).error().map(|e| e.span.clone()).filter(|r| r.start < r.end && text.is_char_boundary(r.start) && text.is_char_boundary(r.end));
        let font_id = size.map_or_else(|| egui::TextStyle::Body.resolve(ui.style()), egui::FontId::proportional);
        let plain = egui::TextFormat { font_id: font_id.clone(), color: ui.visuals().text_color(), ..Default::default() };
        let mut job = egui::text::LayoutJob::default();
        job.wrap.max_width = wrap_width;
        match span {
            Some(r) => {
                let bad = egui::TextFormat { underline: egui::Stroke::new(1.5, crate::theme::palette().link_missing), ..plain.clone() };
                for (part, format) in [(&text[..r.start], plain.clone()), (&text[r.clone()], bad), (&text[r.end..], plain)] {
                    if !part.is_empty() {
                        job.append(part, 0.0, format);
                    }
                }
            }
            None => job.append(text, 0.0, plain),
        }
        ui.ctx().fonts_mut(|f| f.layout_job(job))
    }
}

/// What the search says under its box besides the list: why the query is refused (QUERY-008),
/// the sources it was not asked of (QUERY-006) and that it ended early (QUERY-010).
pub fn notices(ui: &mut Ui, error: Option<&QueryError>, skipped: &[String], stopped_early: bool) {
    if let Some(e) = error {
        crate::theme::problem(ui, e.reason.clone());
    }
    for s in skipped {
        crate::theme::hint(ui, s.clone());
    }
    if stopped_early {
        crate::theme::hint(ui, STOPPED_EARLY);
    }
}

/// The row of count buttons (NAV-052, NAV-053): "Notes 12 · Mail 20+ · Posts 4", the listed ones
/// lit and the others grey, counted all the same. A click solos a source (again: back), Shift+click
/// switches it in or out of the default selection of `cfg`. True when `cfg` changed (save it).
pub fn bar(ui: &mut Ui, bar: &Bar, filter: &mut Filter, cfg: &mut SearchConfig) -> bool {
    let mut changed = false;
    let default = cfg.selection();
    ui.horizontal_wrapped(|ui| {
        for kind in Bar::available(bar.mail_open) {
            let label = bar.counts.map_or_else(|| kind.group().to_string(), |c| c.text(kind, bar.limit));
            let on = bar.listed.contains(kind);
            let text = RichText::new(label).size(12.0).color(if on { crate::theme::palette().text_strong } else { crate::theme::palette().muted });
            let mut tip = format!("Click: list only {}, again: back. Shift+click: switch {} in or out of the default selection.", kind.group(), kind.group());
            if default.toggled(kind).is_none() {
                tip.push_str(" The last selected source cannot be switched off.");
            }
            if bar.prefixed {
                tip = "The prefix in the query chooses the sources.".into();
            }
            let resp = ui.add_enabled(!bar.prefixed, egui::Button::new(text).selected(on)).on_hover_text(tip);
            if resp.clicked() {
                if ui.input(|i| i.modifiers.shift) {
                    if let Some(sel) = default.toggled(kind) {
                        cfg.set_selection(sel);
                        changed = true;
                    }
                } else {
                    filter.click(kind);
                }
            }
        }
        if bar.searching {
            ui.add(egui::Spinner::new().size(12.0)).on_hover_text("Searching…");
        }
    });
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p() -> Prefixes {
        Prefixes::default()
    }

    // spec: QUERY-008, QUERY-013
    #[test]
    fn the_text_box_underlines_the_error_of_the_query_and_parses_once_per_text() {
        let ctx = egui::Context::default();
        let mut cache = Cache::default();
        let sections = |text: &str, cache: &mut Cache| {
            let mut out = Vec::new();
            let mut run = ctx.run_ui(egui::RawInput::default(), |ui| {
                let mut layout = error_layouter(cache, p(), Some(14.0));
                let galley = layout(ui, &text.to_string(), 300.0);
                out = galley.job.sections.iter().map(|s| (s.byte_range.start.0..s.byte_range.end.0, s.format.underline.width > 0.0)).collect::<Vec<_>>();
            });
            run.textures_delta.clear();
            out
        };
        assert_eq!(sections("a AND", &mut cache), [(0..2, false), (2..5, true)], "the operator that has no term after it");
        assert_eq!(sections("a AND", &mut cache).len(), 2);
        assert_eq!(cache.parses, 1, "the same text is not parsed again");
        assert_eq!(sections("a AND b", &mut cache), [(0..7, false)], "a valid query is not underlined");
        assert_eq!(sections("x (y", &mut cache), [(0..2, false), (2..3, true), (3..4, false)], "the paren that is not closed");
        assert_eq!(cache.parses, 3);
        assert_eq!(sections("", &mut cache).iter().filter(|s| s.1).count(), 0);
    }

    // spec: QUERY-006, QUERY-008, QUERY-010
    #[test]
    fn the_notices_say_why_the_query_is_refused_what_was_skipped_and_that_the_search_stopped() {
        let ctx = egui::Context::default();
        let texts = |error: Option<&QueryError>, skipped: &[String], stopped: bool| {
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| notices(ui, error, skipped, stopped));
            out.textures_delta.clear();
            out.shapes
                .iter()
                .filter_map(|s| match &s.shape {
                    egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        assert!(texts(None, &[], false).is_empty(), "nothing to say");
        let error = QueryError { span: 0..1, reason: "this ( is not closed".into() };
        assert_eq!(texts(Some(&error), &["Mail: `status:` not supported".into()], true), ["this ( is not closed", "Mail: `status:` not supported", STOPPED_EARLY]);
    }

    // spec: NAV-050
    #[test]
    fn a_prefix_at_the_start_names_the_sources_with_or_without_a_space() {
        let sel = |text: &str| p().parse(text).prefix.map(|s| s.kinds().collect::<Vec<_>>());
        assert_eq!(sel("f:banana"), Some(vec![Kind::Notes]));
        assert_eq!(sel("m:banana"), Some(vec![Kind::Mail]));
        assert_eq!(sel("s:banana"), Some(vec![Kind::Posts]));
        assert_eq!(sel("f: banana"), Some(vec![Kind::Notes]));
        assert_eq!(sel("  m:  banana"), Some(vec![Kind::Mail]), "blanks before it do not count");
        assert_eq!(sel("fm:banana"), Some(vec![Kind::Notes, Kind::Mail]), "letters combine");
        assert_eq!(sel("sf: x"), Some(vec![Kind::Notes, Kind::Posts]), "in any order");
        assert_eq!(sel("F:banana"), Some(vec![Kind::Notes]), "case is ignored");
        assert_eq!(p().parse("f: ban ana").text, "ban ana");
        assert_eq!(p().parse("m:").text, "", "a prefix alone is an empty query");
        // not a prefix: text
        assert_eq!(sel("banana"), None);
        assert_eq!(sel("a f:banana"), None, "only at the start");
        assert_eq!(sel("ff:banana"), None, "a letter once");
        assert_eq!(sel("x:banana"), None, "no source has x");
        assert_eq!(sel("fx:banana"), None, "one stranger and it is text");
        assert_eq!(sel(":banana"), None);
        assert_eq!(sel("from:ann"), None, "a field of mail and posts goes to them");
        assert_eq!(sel("subject:x"), None);
        assert_eq!(p().parse("from:ann").text, "from:ann");
        assert_eq!(sel("f :banana"), None, "no space before the colon");
    }

    // spec: NAV-050, CFG-067
    #[test]
    fn the_letters_are_the_configured_ones() {
        let q = Prefixes { notes: 'n', mail: 'e', posts: 'p' };
        assert_eq!(q.parse("n:x").prefix, Some(Selection::only(Kind::Notes)));
        assert_eq!(q.parse("ep:x").prefix.map(|s| s.kinds().collect::<Vec<_>>()), Some(vec![Kind::Mail, Kind::Posts]));
        assert_eq!(q.parse("f:x").prefix, None, "the old letters are text now");
        let mut c = SearchConfig { prefix_notes: "n".into(), prefix_mail: "E".into(), prefix_posts: "p".into(), ..Default::default() };
        assert_eq!(c.prefixes(), Prefixes { notes: 'n', mail: 'e', posts: 'p' }, "read as lower case");
        c.prefix_posts = "n".into();
        assert_eq!(c.prefixes(), Prefixes::default(), "letters that are refused: the defaults");
    }

    // spec: NAV-051
    #[test]
    fn letters_that_clash_or_spell_a_field_are_refused() {
        assert_eq!(check_prefixes("f", "m", "s"), Ok(Prefixes::default()));
        assert!(check_prefixes("f", "f", "s").unwrap_err().contains("prefix of notes and of mail"));
        assert!(check_prefixes("f", "m", "m").is_err());
        assert!(check_prefixes("", "m", "s").is_err(), "one letter, not none");
        assert!(check_prefixes("fm", "m", "s").is_err(), "one letter, not two");
        assert!(check_prefixes("1", "m", "s").is_err(), "a letter");
        // letters combine (fm:), so two of them can spell a field: i and n make in:, t and o make to:
        assert!(check_prefixes("i", "n", "s").unwrap_err().contains("“in:”"), "{:?}", check_prefixes("i", "n", "s"));
        assert!(check_prefixes("t", "o", "s").unwrap_err().contains("“to:”"));
        assert!(check_prefixes("f", "r", "o").is_ok(), "from: needs four letters, and there are three");
        assert!(check_prefixes("n", "i", "z").is_err(), "the order does not matter");
        assert_eq!(check_prefixes("c", "e", "z"), Ok(Prefixes { notes: 'c', mail: 'e', posts: 'z' }), "cc has a letter twice: no clash");
        assert!(check_prefixes("x", "t", "e").is_ok(), "text has t twice: no clash");
    }

    // spec: NAV-052
    #[test]
    fn a_solo_is_a_click_that_comes_back_and_moves_on() {
        let mut f = Filter::default();
        let all = Selection::ALL;
        assert_eq!(f.shown(all), all);
        f.click(Kind::Mail);
        assert_eq!(f.shown(all).kinds().collect::<Vec<_>>(), [Kind::Mail]);
        f.click(Kind::Posts);
        assert_eq!(f.solo, Some(Kind::Posts), "another one while soloed: solo that");
        f.click(Kind::Posts);
        assert_eq!(f.solo, None, "again: back");
        let default = Selection::only(Kind::Notes).with(Kind::Posts);
        assert_eq!(f.shown(default), default, "back is the default selection, not all");
    }

    // spec: NAV-053
    #[test]
    fn the_default_selection_cannot_become_empty() {
        let all = Selection::ALL;
        let two = all.toggled(Kind::Mail).unwrap();
        assert_eq!(two.kinds().collect::<Vec<_>>(), [Kind::Notes, Kind::Posts]);
        assert_eq!(two.toggled(Kind::Mail), Some(all), "and in again");
        let one = two.toggled(Kind::Notes).unwrap();
        assert_eq!(one.toggled(Kind::Posts), None, "the last one stays");
        assert_eq!(one.toggled(Kind::Notes).map(|s| s.kinds().count()), Some(2));
    }

    // spec: NAV-055
    #[test]
    fn tab_steps_through_the_sources_and_back_to_the_default() {
        let mut f = Filter::default();
        let a = Bar::available(true);
        let mut seen = vec![];
        for _ in 0..4 {
            f.step(true, &a);
            seen.push(f.solo);
        }
        assert_eq!(seen, [Some(Kind::Notes), Some(Kind::Mail), Some(Kind::Posts), None], "Notes, Mail, Posts, then the default again");
        seen.clear();
        for _ in 0..4 {
            f.step(false, &a);
            seen.push(f.solo);
        }
        assert_eq!(seen, [Some(Kind::Posts), Some(Kind::Mail), Some(Kind::Notes), None], "the same backwards");
        // no mail store: no mail stop
        let a = Bar::available(false);
        assert_eq!(a, [Kind::Notes, Kind::Posts]);
        f.step(true, &a);
        f.step(true, &a);
        assert_eq!(f.solo, Some(Kind::Posts));
        f.solo = Some(Kind::Mail);
        f.step(true, &a);
        assert_eq!(f.solo, Some(Kind::Notes), "a solo on a source the view has not: as none");
    }

    // spec: NAV-054, CFG-067
    #[test]
    fn the_selection_is_words_in_the_settings_and_an_empty_or_unknown_list_is_all() {
        let mut c = SearchConfig::default();
        assert_eq!(c.selection(), Selection::ALL);
        c.set_selection(Selection::only(Kind::Mail).with(Kind::Posts));
        assert_eq!(c.sources, ["mail", "posts"]);
        assert_eq!(c.selection().kinds().collect::<Vec<_>>(), [Kind::Mail, Kind::Posts]);
        for bad in [vec![], vec!["nope".to_string()], vec![String::new()]] {
            c.sources = bad;
            assert_eq!(c.selection(), Selection::ALL);
        }
        c.sources = vec!["Posts".into(), "x".into()];
        assert_eq!(c.selection(), Selection::only(Kind::Posts), "unknown words are left out");
    }

    #[test]
    fn counts_have_a_plus_at_the_limit() {
        let c = Counts([12, 20, 0]);
        assert_eq!((c.text(Kind::Notes, 20), c.text(Kind::Mail, 20), c.text(Kind::Posts, 20)), ("Notes 12".to_string(), "Mail 20+".to_string(), "Posts 0".to_string()));
        assert_eq!(c.text(Kind::Mail, 200), "Mail 20");
    }
}
