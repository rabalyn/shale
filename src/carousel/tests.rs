//! The carousel syntax and what the exports make of it (EDIT-142, PDF-040).

use super::*;

const NOTE: &str = "Intro\n\n> [!carousel] Layers\n> ![[base.png|Base layer]]\n> ![[nav.png|Navigation|300]]\n>\n> ![alt text](sym.png)\n> ![[plain.png]]\n\nAfter\n";

// spec: EDIT-142
#[test]
fn edit_142_a_carousel_is_a_carousel_callout_followed_by_quote_lines_of_image_embeds() {
    let found = find(NOTE);
    assert_eq!(found.len(), 1);
    let c = &found[0];
    assert_eq!(c.title.as_deref(), Some("Layers"));
    // the callout line and the lines to the last image, the empty quote line inside
    assert_eq!(c.lines, 2..8);
    let targets: Vec<&str> = c.slides.iter().map(|s| s.target.as_str()).collect();
    assert_eq!(targets, ["base.png", "nav.png", "sym.png", "plain.png"]);
}

// spec: EDIT-142
#[test]
fn edit_142_the_caption_is_the_alias_without_its_size_or_the_alt_text() {
    let c = &find(NOTE)[0];
    let captions: Vec<Option<&str>> = c.slides.iter().map(|s| s.caption.as_deref()).collect();
    assert_eq!(captions, [Some("Base layer"), Some("Navigation"), Some("alt text"), None]);
    assert_eq!(c.slides[1].width, Some(300.0));
    assert_eq!(c.slides[0].width, None);
    // a size alone is no caption
    let c = &find("> [!carousel]\n> ![[a.png|120]]\n")[0];
    assert_eq!((c.slides[0].caption.clone(), c.slides[0].width), (None, Some(120.0)));
    assert_eq!(c.title, None, "no title");
}

// spec: EDIT-142
#[test]
fn edit_142_the_callout_word_is_not_case_sensitive_and_takes_the_fold_marks_of_other_editors() {
    for head in ["> [!Carousel] T", "> [!CAROUSEL]- T", "  > [!carousel]+ T", ">[!carousel] T"] {
        let c = find(&format!("{head}\n> ![[a.png]]\n"));
        assert_eq!(c.len(), 1, "{head}");
        assert_eq!(c[0].title.as_deref(), Some("T"), "{head}");
    }
}

// spec: EDIT-142
#[test]
fn edit_142_any_other_quote_stays_a_quote() {
    for text in [
        "> [!note] Layers\n> ![[a.png]]\n",
        "> Layers\n> ![[a.png]]\n",
        "> [!carousel] Layers\n",
        "> [!carousel] Layers\n> only words\n",
        "> [!carousel] Layers\n\n> ![[a.png]]\n",
        "> [!carousel] Layers\n> ![](https://example.com/a.png)\n",
        "```\n> [!carousel] Layers\n> ![[a.png]]\n```\n",
        "[!carousel] Layers\n![[a.png]]\n",
    ] {
        assert!(find(text).is_empty(), "{text:?}");
        assert_eq!(flatten(text), text, "{text:?} is exported as written");
    }
}

// spec: EDIT-142
#[test]
fn edit_142_a_line_that_is_not_an_image_ends_the_carousel_and_the_rest_of_the_quote_stays() {
    let c = &find("> [!carousel] T\n> ![[a.png]]\n> ![[b.png]] and words\n> ![[c.png]]\n")[0];
    assert_eq!(c.lines, 0..2);
    assert_eq!(c.slides.len(), 1);
    // two carousels in a note, each with its own lines
    let two = find("> [!carousel] A\n> ![[a.png]]\n\n> [!carousel] B\n> ![[b.png]]\n> ![[c.png]]\n");
    assert_eq!(two.iter().map(|c| (c.lines.clone(), c.slides.len())).collect::<Vec<_>>(), [(0..2, 1), (3..6, 2)]);
}

// spec: PDF-040
#[test]
fn pdf_040_the_exports_get_every_image_in_order_each_alone_with_its_caption_below() {
    let flat = flatten(NOTE);
    let want = "Intro\n\n**Layers**\n\n![[base.png]]\n\n*Base layer*\n\n![[nav.png|300]]\n\n*Navigation*\n\n![](sym.png)\n\n*alt text*\n\n![[plain.png]]\n\nAfter\n";
    assert_eq!(flat, want);
    // the flat text has no carousel any more: flattening twice changes nothing
    assert_eq!(flatten(&flat), flat);
}

// spec: PDF-040
#[test]
fn pdf_040_a_carousel_between_lines_of_text_is_set_apart_by_blank_lines() {
    let flat = flatten("before\n> [!carousel]\n> ![[a.png|A]]\nafter\n");
    assert_eq!(flat, "before\n\n![[a.png]]\n\n*A*\n\nafter\n");
    // the front matter and what is not a carousel are untouched
    let text = "---\ntitle: x\n---\n> quote\n";
    assert_eq!(flatten(text), text);
}
