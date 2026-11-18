//! Tests of `mail_text::tidy` (IMP-053, IMP-055).

use super::mail_text::{tidy, SIGNATURE_DELIMITER};

// spec: IMP-053
#[test]
fn runs_of_blank_lines_trailing_spaces_and_line_ends_are_tidied() {
    assert_eq!(tidy("Hallo  \r\n\r\n\r\n\r\n\r\nWelt \t\r\n"), "Hallo\n\nWelt");
    assert_eq!(tidy("\n\n a\n\n\nb\n\n\n\nc\n\n\n\n"), " a\n\n\nb\n\nc", "two blank lines stay, three or more become one; the ends are trimmed");
    assert_eq!(tidy("a\n>\n>\n>\n>\nb"), "a\n>\n>\n>\n>\nb", "a quote's empty `>` lines are not blank lines");
    assert_eq!(tidy(""), "");
}

// spec: IMP-053
#[test]
fn a_long_url_with_a_query_becomes_a_link_with_its_host() {
    let long = format!("https://www.news.example.org/track/click?uid={}&u=abc", "x".repeat(90));
    assert_eq!(tidy(&format!("Hier: {long}. Danke")), format!("Hier: [news.example.org]({long}). Danke"));
    assert_eq!(tidy(&format!("<{long}>")), format!("[news.example.org]({long})"), "the brackets of a plain-text mail go");
    assert_eq!(tidy(&format!("> see {long}")), format!("> see [news.example.org]({long})"), "also in a quote");
    assert_eq!(tidy(&format!("(see {long})")), format!("(see [news.example.org]({long}))"), "the bracket that closes nothing is not the address's");
    assert_eq!(tidy(&format!("[Zur Seite]({long})")), format!("[Zur Seite]({long})"), "a Markdown link is left alone");
    let short = "https://example.org/a?b=1";
    assert_eq!(tidy(short), short, "a short URL stays");
    let no_query = format!("https://example.org/{}", "p/".repeat(60));
    assert_eq!(tidy(&no_query), no_query, "a long URL without a query stays");
    let bracket = long.replace("&u=abc", "&u=(a)");
    assert_eq!(tidy(&bracket), format!("[news.example.org]({})", bracket.replace('(', "%28").replace(')', "%29")), "brackets in the address cannot end the link");
}

// spec: IMP-055
#[test]
fn the_signature_delimiter_stays_exactly_and_stands_alone() {
    assert_eq!(SIGNATURE_DELIMITER, "-- ");
    assert_eq!(tidy("Gruß\n-- \nHans  \nBau B  \n"), "Gruß\n\n-- \nHans\nBau B", "the line `-- ` keeps its space; a blank line comes before it");
    assert_eq!(tidy("Gruß\n\n--\nHans"), "Gruß\n\n-- \nHans", "a bare `--` becomes the delimiter");
    assert_eq!(tidy("Gruß\n\n\n\n-- \nHans"), "Gruß\n\n-- \nHans");
    assert_eq!(tidy("> zitat\n> -- \n> Hans\n\nAntwort"), "> zitat\n> --\n> Hans\n\nAntwort", "a quoted delimiter is a quote like any other");
    assert_eq!(tidy("a\n---\nb"), "a\n---\nb", "a rule is no delimiter");
}
