//! Tests of the depth limit for XML from outside (DEV-038). None of them needs a program or a file.

use super::xml_limits::{check_depth, parse, MAX_DEPTH};

fn nested(n: usize) -> String {
    format!("<?xml version=\"1.0\"?>\n{}x{}", "<a>".repeat(n), "</a>".repeat(n))
}

// spec: DEV-038
#[test]
fn a_document_nested_too_deeply_is_an_error_and_the_deepest_allowed_parses_on_a_small_stack() {
    let e = parse(&nested(MAX_DEPTH + 1)).unwrap_err();
    assert_eq!(e, format!("the XML is nested too deeply (more than {MAX_DEPTH} levels)"));
    // documents that would abort the process are refused, not parsed
    assert!(parse(&nested(1_000_000)).is_err());
    assert!(parse(&"<a>".repeat(1_000_000)).is_err(), "not even closed");
    // the deepest that is allowed is read on a quarter of the usual 2 MiB test thread, so that the limit has room
    let text = nested(MAX_DEPTH);
    let read = std::thread::Builder::new().stack_size(512 * 1024).spawn(move || parse(&text).map(|d| d.descendants().count())).unwrap().join().expect("no stack overflow at the deepest level that is allowed");
    assert_eq!(read, Ok(MAX_DEPTH + 2), "the elements, the text and the root");
    assert!(parse("<a/>").is_ok() && parse("").is_err(), "roxmltree's own errors stay");
}

// spec: DEV-038
#[test]
fn only_tags_count_for_the_depth() {
    let many = |inner: &str| format!("<r>{}</r>", inner.repeat(MAX_DEPTH * 2));
    for (what, inner) in [
        ("empty elements", "<e/>"),
        ("empty elements with attributes", "<e a=\"1\" b='2' />"),
        ("comments", "<!-- <a><a><a> -->"),
        ("CDATA", "<![CDATA[<a><a><a>]]>"),
        ("processing instructions", "<?pi <a> ?>"),
        ("closed elements", "<e></e>"),
        ("a > in an attribute", "<e a=\">\"/>"),
    ] {
        assert!(check_depth(&many(inner)).is_ok(), "{what}");
        assert!(parse(&many(inner)).is_ok(), "{what}");
    }
    // not well-formed (a < in an attribute value), but the scan is not fooled by the tags inside it
    assert!(check_depth(&format!("<r>{}</r>", "<e a='<a><a>'/>".repeat(MAX_DEPTH * 2))).is_ok());
    // a declaration with an internal subset does not count either, and what follows it does
    let doctype = "<!DOCTYPE r [<!ENTITY e \"<a><a>\"> <!-- ] --> ]><r/>";
    assert!(check_depth(doctype).is_ok());
    let deep = format!("<!DOCTYPE r [<!ELEMENT r ANY>]>{}", "<a>".repeat(MAX_DEPTH + 1));
    assert!(check_depth(&deep).is_err());
    // each closing tag lifts the depth: many siblings are no depth
    assert!(check_depth(&format!("<r>{}</r>", "<a><b><c></c></b></a>".repeat(10_000))).is_ok());
    // a closing tag without a start does not make the count wrap round
    assert!(check_depth(&format!("{}{}", "</a>".repeat(10), "<a>".repeat(MAX_DEPTH))).is_ok());
    assert!(check_depth(&format!("{}{}", "</a>".repeat(10), "<a>".repeat(MAX_DEPTH + 1))).is_err());
    assert!(check_depth("<a>é<b>ü</b>€</a>").is_ok(), "letters beyond ASCII");
}

// spec: DEV-038
#[test]
fn what_is_not_well_formed_is_left_to_roxmltree_and_never_loops() {
    for text in ["<", "<a", "<a b=\"x", "<a b='x", "<!--", "<!-- x", "<![CDATA[", "<?", "<!", "<!DOCTYPE x [", "<!DOCTYPE x [ \"", "</", "<a></", "x<", "<a><", "<é"] {
        assert!(check_depth(text).is_ok(), "{text:?}");
        assert!(parse(text).is_err(), "{text:?}");
    }
}
