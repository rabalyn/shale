//! Citations, references and labels in the editor (EDIT-124, EDIT-125, EDIT-126, #646): the list of
//! keys and labels while typing, the peek, and the marks of what does not resolve. The document
//! (its bibliography in effect, its labels) is built from texts, so these frames need no job to settle.

use super::tests::{vault_for_tests, Harness};
use super::*;
use crate::thesis::View;
use egui::{Event, PointerButton};

const BIB: &str = "@article{knuth1984, author = {Knuth, Donald E.}, title = {Literate Programming}, journaltitle = {The Computer Journal}, date = {1984}}\n@book{lamport1994, author = {Lamport, Leslie}, title = {LaTeX}, publisher = {Addison-Wesley}, date = {1994}}\n";

fn key(key: Key) -> Event {
    Event::Key { key, physical_key: Some(key), pressed: true, repeat: false, modifiers: Modifiers::NONE }
}

/// The document of `text` with the bibliography `bib` in effect (or none).
fn document(text: &str, bib: Option<&str>) -> Arc<View> {
    let effect = bib.map(crate::thesis::view::test_effect).unwrap_or_default();
    Arc::new(View::build(None, &[text], 0, text, effect, false))
}

/// An editor with focus whose document is that of `doc`, and a harness whose tooltips need no delay.
fn setup(tag: &str, doc: &str, bib: Option<&str>) -> (NoteEditor, Harness, Vault, std::path::PathBuf) {
    let (v, d) = vault_for_tests(tag);
    let mut ed = NoteEditor::new(egui::Id::unique(tag.to_string()));
    ed.request_focus = true;
    ed.thesis = document(doc, bib);
    let h = Harness { ctx: egui::Context::default(), time: 0.0, texts: vec![], rects: vec![] };
    h.ctx.all_styles_mut(|s| s.interaction.tooltip_delay = 0.0);
    (ed, h, v, d)
}

fn items(ed: &NoteEditor) -> Vec<String> {
    ed.assist.cites.items.iter().map(|c| c.insert.clone()).collect()
}

/// The colour the layout of the last frame gives the first `word`.
fn colour_of(ed: &NoteEditor, text: &str, word: &str) -> egui::Color32 {
    let (galley, _) = ed.last_layout.clone().expect("a frame was laid out");
    let at = text.find(word).unwrap_or_else(|| panic!("{word:?} is not in the text"));
    let section = galley.job.sections.iter().find(|s| s.byte_range.contains(&egui::text::ByteIndex(at))).expect("a section");
    section.format.color
}

/// The point on the first `word` of the text, as painted in the last frame.
fn point_on(ed: &NoteEditor, text: &str, word: &str) -> Pos2 {
    let (galley, pos) = ed.last_layout.clone().expect("a frame was laid out");
    let ch = markdown::byte_to_char(text, text.find(word).unwrap_or_else(|| panic!("{word:?} is not in the text"))) + 2;
    pos + galley.pos_from_cursor(CCursor::new(ch)).center().to_vec2()
}

// spec: EDIT-124
#[test]
fn edit_124_after_a_bracket_the_keys_are_listed_with_authors_year_and_title_and_enter_picks_one() {
    let (mut ed, mut h, v, d) = setup("c124a", "", Some(BIB));
    let (rel, mut text) = (Path::new("Welcome.md"), String::new());
    h.frame(&mut ed, &mut text, &v, rel, vec![]);
    h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("see [@kn".into())]);
    h.frame(&mut ed, &mut text, &v, rel, vec![]);
    assert!(ed.assist.cites.active && ed.assist.is_open());
    assert_eq!(items(&ed), ["knuth1984"], "what is typed picks the keys");
    assert_eq!(ed.assist.cites.items[0].detail, "Knuth (1984): Literate Programming. The Computer Journal");
    // a word of the authors or the title finds a key as well
    h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::Backspace), key(Key::Backspace)]);
    h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("Lamp".into())]);
    h.frame(&mut ed, &mut text, &v, rel, vec![]);
    assert_eq!(items(&ed), ["lamport1994"], "the key matches `Lamp` by its letters in order");
    h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::Backspace), key(Key::Backspace), key(Key::Backspace), key(Key::Backspace)]);
    h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("Literate".into())]);
    h.frame(&mut ed, &mut text, &v, rel, vec![]);
    assert_eq!(items(&ed), ["knuth1984"], "a word of the title finds the key");
    h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::Enter)]);
    assert_eq!(text, "see [@knuth1984");
    assert!(!ed.assist.cites.active, "the list closes after the pick");
    let _ = std::fs::remove_dir_all(&d);
}

// spec: EDIT-124
#[test]
fn edit_124_a_semicolon_a_space_and_a_bare_at_open_the_list_while_a_bibliography_is_in_effect() {
    let (mut ed, mut h, v, d) = setup("c124b", "", Some(BIB));
    let (rel, mut text) = (Path::new("Welcome.md"), String::new());
    h.frame(&mut ed, &mut text, &v, rel, vec![]);
    h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("[@knuth1984; @la".into())]);
    h.frame(&mut ed, &mut text, &v, rel, vec![]);
    assert_eq!(items(&ed), ["lamport1994"]);
    h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::Enter)]);
    assert_eq!(text, "[@knuth1984; @lamport1994");
    h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("] and @kn".into())]);
    h.frame(&mut ed, &mut text, &v, rel, vec![]);
    assert_eq!(items(&ed), ["knuth1984"], "a bare @ is a citation while a bibliography holds keys");
    // an address is no citation
    h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::Escape), Event::Text(" ada@kn".into())]);
    h.frame(&mut ed, &mut text, &v, rel, vec![]);
    assert!(!ed.assist.cites.active, "ada@kn is no query");
    let _ = std::fs::remove_dir_all(&d);
}

// spec: EDIT-124
#[test]
fn edit_124_without_a_bibliography_a_bare_at_lists_nothing_and_a_bracket_says_so_and_offers_the_setting() {
    let (mut ed, mut h, v, d) = setup("c124c", "", None);
    let (rel, mut text) = (Path::new("Welcome.md"), String::new());
    h.frame(&mut ed, &mut text, &v, rel, vec![]);
    h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("mail @kn".into())]);
    h.frame(&mut ed, &mut text, &v, rel, vec![]);
    assert!(!ed.assist.cites.is_open(), "a bare @ is plain text without a bibliography");
    h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text(" [@kn".into())]);
    h.frame(&mut ed, &mut text, &v, rel, vec![]);
    assert!(!ed.assist.cites.active);
    assert_eq!(ed.assist.cites.notice.as_deref(), Some("No bibliography is in effect for this note."));
    assert!(ed.assist.is_open());
    // Enter is a new line then, not a pick
    h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::Enter)]);
    assert!(text.ends_with("[@kn\n"), "{text:?}");
    let _ = std::fs::remove_dir_all(&d);
}

// spec: EDIT-124
#[test]
fn edit_124_after_a_label_prefix_the_labels_of_the_document_are_listed_with_number_and_caption() {
    let doc = "![[a.png|Results of run 2]]{#fig:results}\n\n![[b.png|Other]]{#fig:other}\n\n| a |\n|---|\n| 1 |\n: Runs {#tbl:runs}\n\n";
    let (mut ed, mut h, v, d) = setup("c124d", doc, None);
    let rel = Path::new("Welcome.md");
    let mut text = doc.to_string();
    h.frame(&mut ed, &mut text, &v, rel, vec![Event::PointerMoved(Pos2::new(5.0, 5.0))]);
    ed.pending_cursor = Some(text.chars().count());
    h.frame(&mut ed, &mut text, &v, rel, vec![]);
    h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("See @fig:re".into())]);
    h.frame(&mut ed, &mut text, &v, rel, vec![]);
    assert_eq!(items(&ed), ["fig:results"], "only figures, matched by what is typed: {:?}", ed.assist.cites.items);
    assert_eq!(ed.assist.cites.items[0].detail, "Figure 1 · Results of run 2");
    h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::Enter)]);
    assert!(text.ends_with("See @fig:results"), "{text:?}");
    h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text(", @tbl:".into())]);
    h.frame(&mut ed, &mut text, &v, rel, vec![]);
    assert_eq!(items(&ed), ["tbl:runs"]);
    assert_eq!(ed.assist.cites.items[0].detail, "Table 1 · Runs");
    let _ = std::fs::remove_dir_all(&d);
}

// spec: EDIT-124
#[test]
fn edit_124_escape_closes_the_list_for_this_key_and_enter_is_a_new_line_again() {
    let (mut ed, mut h, v, d) = setup("c124e", "", Some(BIB));
    let (rel, mut text) = (Path::new("Welcome.md"), String::new());
    h.frame(&mut ed, &mut text, &v, rel, vec![]);
    h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("[@kn".into())]);
    h.frame(&mut ed, &mut text, &v, rel, vec![]);
    assert!(ed.assist.cites.active);
    h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::Escape)]);
    h.frame(&mut ed, &mut text, &v, rel, vec![]);
    assert!(!ed.assist.cites.active);
    h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::Enter)]);
    assert_eq!(text, "[@kn\n", "nothing was picked");
    let _ = std::fs::remove_dir_all(&d);
}

const MARKED: &str = "A [@knuth1984; @nobody] and @fig:zz, @fig:ok and @todo.\n\n![[a.png|A plot]]{#fig:ok}\n\n![[b.png|B]]{#fig:ok}\n";

// spec: EDIT-126
#[test]
fn edit_126_an_unknown_key_a_missing_label_and_a_second_label_are_drawn_like_a_broken_link() {
    let (mut ed, mut h, v, d) = setup("c126a", MARKED, Some(BIB));
    let rel = Path::new("Welcome.md");
    let mut text = MARKED.to_string();
    h.frame(&mut ed, &mut text, &v, rel, vec![]);
    let p = theme::palette();
    assert_eq!(colour_of(&ed, &text, "@knuth1984"), p.link, "a key of the bibliography is drawn like a link that resolves");
    assert_eq!(colour_of(&ed, &text, "@nobody"), p.link_missing, "an unknown key is red");
    assert_eq!(colour_of(&ed, &text, "@fig:zz"), p.link_missing, "a label that does not exist is red");
    assert_eq!(colour_of(&ed, &text, "@fig:ok"), p.link, "one that does is not");
    assert_eq!(colour_of(&ed, &text, "@todo"), p.text, "a bare word that is no key is no citation and not marked");
    let second = text.rfind("{#fig:ok}").unwrap();
    let section = ed.last_layout.clone().unwrap().0.job.sections.iter().find(|s| s.byte_range.contains(&egui::text::ByteIndex(second))).unwrap().format.color;
    assert_eq!(section, p.link_missing, "the second label is a duplicate");
    assert_ne!(colour_of(&ed, &text, "{#fig:ok}"), p.link_missing, "the first one counts");
    let _ = std::fs::remove_dir_all(&d);
}

// spec: EDIT-126
#[test]
fn edit_126_without_a_bibliography_a_group_is_plain_text_and_the_references_are_still_checked() {
    let (mut ed, mut h, v, d) = setup("c126b", MARKED, None);
    let rel = Path::new("Welcome.md");
    let mut text = MARKED.to_string();
    h.frame(&mut ed, &mut text, &v, rel, vec![]);
    let p = theme::palette();
    assert_eq!(colour_of(&ed, &text, "@knuth1984"), p.text, "no bibliography, no mark");
    assert_eq!(colour_of(&ed, &text, "@nobody"), p.text);
    assert_eq!(colour_of(&ed, &text, "@fig:zz"), p.link_missing, "the prefixes always make a reference");
    let _ = std::fs::remove_dir_all(&d);
}

/// Hover the pointer at `p` for a few frames, so that the tooltip is up, and the texts painted then.
fn hover(h: &mut Harness, ed: &mut NoteEditor, text: &mut String, v: &Vault, rel: &Path, p: Pos2) -> Vec<String> {
    h.frame(ed, text, v, rel, vec![Event::PointerMoved(p)]);
    for _ in 0..3 {
        h.frame(ed, text, v, rel, vec![]);
    }
    h.texts.clone()
}

// spec: EDIT-125
#[test]
fn edit_125_the_pointer_on_a_key_shows_the_entry_and_on_a_reference_its_number_and_caption() {
    let (mut ed, mut h, v, d) = setup("c125a", MARKED, Some(BIB));
    let rel = Path::new("Welcome.md");
    let mut text = MARKED.to_string();
    h.frame(&mut ed, &mut text, &v, rel, vec![]);
    let on = point_on(&ed, &text, "@knuth1984");
    let shown = hover(&mut h, &mut ed, &mut text, &v, rel, on);
    assert!(shown.iter().any(|t| t == "Knuth (1984): Literate Programming. The Computer Journal"), "the entry: {shown:?}");
    let on = point_on(&ed, &text, "@fig:ok");
    let shown = hover(&mut h, &mut ed, &mut text, &v, rel, on);
    assert!(shown.iter().any(|t| t == "Figure 1") && shown.iter().any(|t| t == "A plot"), "number and caption: {shown:?}");
    let on = point_on(&ed, &text, "@nobody");
    let shown = hover(&mut h, &mut ed, &mut text, &v, rel, on);
    assert!(shown.iter().any(|t| t == "“nobody” is not in the bibliography"), "the reason: {shown:?}");
    let on = point_on(&ed, &text, "@fig:zz");
    let shown = hover(&mut h, &mut ed, &mut text, &v, rel, on);
    assert!(shown.iter().any(|t| t == "There is no label {#fig:zz} in the document"), "the reason: {shown:?}");
    let on = point_on(&ed, &text, "@todo");
    let shown = hover(&mut h, &mut ed, &mut text, &v, rel, on);
    assert!(!shown.iter().any(|t| t.contains("not in the bibliography") || t.starts_with("Knuth")), "no peek for a bare word: {shown:?}");
    let _ = std::fs::remove_dir_all(&d);
}

// spec: EDIT-125
#[test]
fn edit_125_ctrl_click_keeps_the_peek_open_until_escape() {
    let (mut ed, mut h, v, d) = setup("c125b", MARKED, Some(BIB));
    let rel = Path::new("Welcome.md");
    let mut text = MARKED.to_string();
    h.frame(&mut ed, &mut text, &v, rel, vec![]);
    let on = point_on(&ed, &text, "@knuth1984");
    let ctrl = Modifiers { command: true, ctrl: true, ..Modifiers::NONE };
    h.frame(&mut ed, &mut text, &v, rel, vec![Event::ModifiersChanged(ctrl), Event::PointerMoved(on), Event::PointerButton { pos: on, button: PointerButton::Primary, pressed: true, modifiers: ctrl }]);
    h.frame(&mut ed, &mut text, &v, rel, vec![Event::PointerButton { pos: on, button: PointerButton::Primary, pressed: false, modifiers: ctrl }]);
    h.frame(&mut ed, &mut text, &v, rel, vec![Event::ModifiersChanged(Modifiers::NONE)]);
    // the pointer leaves: a hover would be gone, the pinned peek is not
    h.frame(&mut ed, &mut text, &v, rel, vec![Event::PointerMoved(Pos2::new(700.0, 600.0))]);
    h.frame(&mut ed, &mut text, &v, rel, vec![]);
    assert!(h.texts.iter().any(|t| t == "Knuth (1984): Literate Programming. The Computer Journal"), "pinned: {:?}", h.texts);
    h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::Escape)]);
    h.frame(&mut ed, &mut text, &v, rel, vec![]);
    assert!(!h.texts.iter().any(|t| t.starts_with("Knuth (1984)")), "Esc closes it: {:?}", h.texts);
    let _ = std::fs::remove_dir_all(&d);
}

// spec: EDIT-127
#[test]
fn edit_127_the_label_after_an_embed_goes_with_the_link_and_the_caption_of_the_picture_has_the_figure_number() {
    let text = "![[a.png|A plot]]{#fig:p}\n\n![[b.png|B]] {#fig:q}\n\n![[c.png|C]]{#tbl:x}\n\n![[d.png]]\n";
    assert_eq!(cite::with_label(text, 0..17), 0..25, "the label right after the embed");
    let second = text.find("![[b.png").unwrap();
    assert_eq!(&text[cite::with_label(text, second..second + 12)], "![[b.png|B]] {#fig:q}", "one space between them");
    let third = text.find("![[c.png").unwrap();
    assert_eq!(cite::with_label(text, third..third + 12), third..third + 12, "the label of a table is none of a figure's");
    let last = text.find("![[d.png").unwrap();
    assert_eq!(cite::with_label(text, last..last + 10), last..last + 10, "no label");
    assert_eq!(cite::with_label(text, 0..1000), 0..1000, "a range that is not in the text is left alone");

    // the picture is shown with its caption "Figure 1: A plot"
    let (v, d) = vault_for_tests("c127a");
    image::RgbaImage::from_pixel(8, 8, image::Rgba([200, 0, 0, 255])).save(d.join("a.png")).unwrap();
    let mut v = v;
    v.rescan();
    let doc = "intro\n![[a.png|A plot]]{#fig:p}\n";
    let mut ed = NoteEditor::new(egui::Id::unique("c127a"));
    ed.inline_images = true;
    ed.thesis = document(doc, None);
    let (rel, mut text) = (Path::new("Welcome.md"), doc.to_string());
    let mut h = Harness { ctx: egui::Context::default(), time: 0.0, texts: vec![], rects: vec![] };
    h.settle(&mut ed, &mut text, &v, rel);
    assert!(h.texts.iter().any(|t| t == "Figure 1: A plot"), "{:?}", h.texts);
    let _ = std::fs::remove_dir_all(&d);
}
