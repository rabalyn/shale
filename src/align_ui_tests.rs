//! Aligned paragraphs on the screen (EDIT-110, PDF-028): the reading view lays the paragraphs and
//! headings of an alignment wrapper out centred, to the right or justified and does not show the
//! wrapper's own lines; the toolbar has the three buttons.

use crate::page_break_ui_tests::read;
use eframe::egui::Rect;

fn find(painted: &[(String, Rect)], text: &str) -> Rect {
    painted.iter().find(|(t, _)| t == text).unwrap_or_else(|| panic!("{text:?} is not painted: {painted:?}")).1
}

// spec: EDIT-110
#[test]
fn the_reading_view_centres_and_right_aligns_what_the_wrapper_holds_and_draws_no_html() {
    let note = "plain\n\n<div align=\"center\">\n\nmiddle\n\n</div>\n\n<div align=\"right\">\n\n## Far right\n\nfar text\n\n</div>\n\nplain again\n";
    let (painted, _) = read(note);
    assert!(painted.iter().all(|(t, _)| !t.contains("<div") && !t.contains("</div>")), "the wrapper lines are not shown: {painted:?}");
    let (plain, middle, head, far, again) = (find(&painted, "plain"), find(&painted, "middle"), find(&painted, "Far right"), find(&painted, "far text"), find(&painted, "plain again"));
    // the column runs from the left edge of the text to the right edge of what is flush right
    let (left, right) = (plain.min.x, far.max.x.max(head.max.x));
    assert!(right - left > 100.0, "{left} {right}");
    assert!((middle.center().x - (left + right) / 2.0).abs() < 2.0, "centred: {middle:?} in {left}..{right}");
    assert!((far.max.x - head.max.x).abs() < 2.0 && head.min.x > left + 50.0 && far.min.x > left + 50.0, "the heading and the text end at the same place on the right: {head:?} {far:?}");
    assert!((again.min.x - left).abs() < 0.5, "after the wrapper the text is left again");
    // the text keeps the order down the page
    assert!(plain.max.y < middle.min.y && middle.max.y < head.min.y && head.max.y < far.min.y && far.max.y < again.min.y);
}

// spec: EDIT-110
#[test]
fn an_opening_line_without_a_closing_one_is_shown_as_text() {
    let (painted, _) = read("<div align=\"center\">\n\nabc\n");
    assert!(painted.iter().any(|(t, _)| t.contains("<div")), "it is text: {painted:?}");
    let (plain, abc) = (find(&painted, "<div align=\"center\">"), find(&painted, "abc"));
    assert!((plain.min.x - abc.min.x).abs() < 0.5, "and nothing is aligned");
}
