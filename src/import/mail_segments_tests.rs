//! Tests of the segments of a mail's text and their signatures (IMP-063 to IMP-066): mails of the
//! shapes that Thunderbird, Outlook, Gmail and Apple Mail write, and of the cases that must stay
//! uncut.

use proptest::prelude::*;

use super::mail_segments::{cut, join, quoted_signatures, signature_start, split, TextCut};
use crate::property_test_support::config;

const SIGNATURES: TextCut = TextCut { skip_signatures: true, leave_out_quoted: false };
const QUOTES: TextCut = TextCut { skip_signatures: false, leave_out_quoted: true };
const BOTH: TextCut = TextCut { skip_signatures: true, leave_out_quoted: true };

/// Thunderbird, plain text, bottom-posted: answers below the quote; delimiters at depth 0, 1 and 2.
const THUNDERBIRD: &str = "Hallo Anna,

On 12.11.2026 09:15, Bob Meier wrote:
> Hi Hans,
>
> can we meet on Friday?
>
> Bob
> --\x20
> Bob Meier
> Acme GmbH
> Tel +49 30 1234567
>
> On 11.11.2026 17:00, Hans wrote:
>> I sent the plan.
>> --\x20
>> Hans Muster
>> Muster AG

Friday is fine.

--\x20
Hans Muster
Muster AG, Hauptstr. 1, 10115 Berlin
";

/// Outlook, top-posted, German and English headers: three signatures and a disclaimer, none with a delimiter.
const OUTLOOK: &str = "Hallo Frau Schmidt,

der Termin passt mir.

Mit freundlichen Grüßen
Hans Muster
Projektleiter
Muster AG
Tel.: +49 30 1234567
www.muster.example

________________________________
Von: Anna Schmidt <anna@example.org>
Gesendet: Donnerstag, 12. November 2026 09:15
An: Hans Muster <hans@muster.example>
Betreff: AW: Termin

Hallo Herr Muster,

passt Freitag?

Viele Grüße
Anna Schmidt
Beispiel GmbH
Hauptstr. 1, 10115 Berlin
anna@example.org

-----Original Message-----
From: Hans Muster
Sent: Wednesday, November 11, 2026 5:00 PM
To: Anna Schmidt
Subject: Termin

When do you have time?

Best regards,
Hans
Muster AG
Mobil: +49 170 1234567

This e-mail may contain confidential information. If you are not the intended recipient, please notify the sender and delete it.
";

/// A German mail without a delimiter.
const GERMAN: &str = "Hallo Frau Schmidt,

anbei die Unterlagen für Freitag. Bitte melden Sie sich, falls etwas fehlt.

Viele Grüße
Hans Muster

Muster AG
Hauptstraße 1
10115 Berlin
Tel.: 030 1234567
hans@muster.example
";

/// Answers between quoted lines; the last one has the closing lines of a mail without a delimiter.
const INLINE: &str = "On Mon, Nov 9, 2026 at 10:00 AM Anna <anna@example.org> wrote:
> Can you send the plan?
Yes, attached.
> And the budget?
It is in the second sheet.

Best regards
Hans
Muster AG
+49 30 1234567
";

/// A `-- ` inside a fenced code block, and a greeting in the middle of the text.
const CODE: &str = "Try this:

```
cat x
--\x20
tail y
```

Grüße gehen an alle, die dabei waren, und ich hoffe, dass wir uns im Frühjahr wiedersehen können.
Danach machen wir weiter mit dem Plan, so wie er im letzten Monat besprochen wurde, ohne Eile.
";

fn seg_shapes(text: &str) -> Vec<(usize, usize)> {
    split(text).iter().map(|s| (s.depth, s.header_lines)).collect()
}

// spec: IMP-063
#[test]
fn a_bottom_posted_thunderbird_mail_splits_into_the_own_text_and_each_quote_by_depth() {
    assert_eq!(seg_shapes(THUNDERBIRD), vec![(0, 0), (1, 1), (2, 1), (0, 0)]);
    let segs = split(THUNDERBIRD);
    assert!(segs[1].text.starts_with("On 12.11.2026 09:15, Bob Meier wrote:\n> Hi Hans,"), "the header line starts its quote: {:?}", segs[1].text);
    assert!(segs[2].text.starts_with("> On 11.11.2026 17:00, Hans wrote:\n>> I sent the plan."));
    assert!(segs[3].text.starts_with("\nFriday is fine.") || segs[3].text.starts_with("Friday is fine."));
    assert_eq!(join(&segs), THUNDERBIRD, "no text is lost");
}

// spec: IMP-063
#[test]
fn an_outlook_mail_splits_at_the_separator_line_the_original_message_line_and_the_header_blocks() {
    assert_eq!(seg_shapes(OUTLOOK), vec![(0, 0), (1, 5), (2, 5)], "the underscore line and the Von block are one header; so are the Original Message line and its From block");
    let segs = split(OUTLOOK);
    assert!(segs[1].text.starts_with("________________________________\nVon: Anna Schmidt"));
    assert!(segs[2].text.starts_with("-----Original Message-----\nFrom: Hans Muster\n"));
    assert_eq!(join(&segs), OUTLOOK);
}

// spec: IMP-063
#[test]
fn the_reply_headers_of_both_languages_and_the_wrapped_form_are_recognised() {
    for h in [
        "On Thu, Nov 12, 2026 at 9:15 AM Anna Schmidt <anna@example.org> wrote:",
        "Am 12.11.2026 um 09:15 schrieb Anna Schmidt <anna@example.org>:",
        "Am Do., 12. Nov. 2026 um 09:15 Uhr schrieb Anna Schmidt:",
        "Le 12 nov. 2026 à 09:15, Anna <anna@example.org> a écrit :",
    ] {
        let t = format!("Ok.\n\n{h}\n> text\n");
        assert_eq!(seg_shapes(&t), vec![(0, 0), (1, 1)], "{h}");
    }
    let wrapped = "Ok.\n\nOn Thu, Nov 12, 2026 at 9:15 AM Anna Schmidt <\nanna@example.org> wrote:\n> text\n";
    assert_eq!(seg_shapes(wrapped), vec![(0, 0), (1, 2)], "a header that Gmail wraps takes two lines");
    assert_eq!(seg_shapes("Ok.\n\n-----Ursprüngliche Nachricht-----\nVon: A\nGesendet: B\nAn: C\nBetreff: D\n\ntext\n"), vec![(0, 0), (1, 5)]);
    assert_eq!(seg_shapes("On the table lies a note that nobody wrote.\n"), vec![(0, 0)], "a sentence is no header");
}

// spec: IMP-063
#[test]
fn a_forwarded_header_block_and_a_lone_from_line_are_no_reply() {
    let forwarded = "FYI\n\n---------- Forwarded message ---------\nFrom: Bob\nDate: Mon\nSubject: Plan\nTo: me\n\nthe plan\n";
    assert_eq!(seg_shapes(forwarded), vec![(0, 0)]);
    assert_eq!(seg_shapes("From: the sea\nto the shore\n"), vec![(0, 0)]);
}

// spec: IMP-063
#[test]
fn answers_between_quoted_lines_are_own_segments_and_quotes_alternate() {
    assert_eq!(seg_shapes(INLINE), vec![(1, 1), (0, 0), (1, 0), (0, 0)]);
    assert_eq!(join(&split(INLINE)), INLINE);
    assert_eq!(seg_shapes("> a\n>\n> > b\n> c\n"), vec![(1, 0), (2, 0), (1, 0)], "nested quotes are deeper segments; empty `>` lines join their neighbours");
    assert_eq!(seg_shapes(">a\n>>b\n"), vec![(1, 0), (2, 0)], "marks without a space");
}

// spec: IMP-063
#[test]
fn an_html_mail_gives_quotes_and_signature_lines_of_its_own_to_the_text() {
    let gmail = "<div dir=\"ltr\">Sounds good, see you Friday.<br clear=\"all\"><div><br></div><span class=\"gmail_signature_prefix\">-- </span><br><div dir=\"ltr\" class=\"gmail_signature\" data-smartmail=\"gmail_signature\"><div dir=\"ltr\">Hans Muster<br>Muster AG</div></div></div><br><div class=\"gmail_quote\"><div dir=\"ltr\" class=\"gmail_attr\">On Thu, Nov 12, 2026 at 9:15 AM Anna Schmidt &lt;<a href=\"mailto:anna@example.org\">anna@example.org</a>&gt; wrote:<br></div><blockquote class=\"gmail_quote\" style=\"margin:0px 0px 0px 0.8ex\"><div dir=\"ltr\">Can we meet on Friday?<br clear=\"all\"><div><br></div><span class=\"gmail_signature_prefix\">-- </span><br><div dir=\"ltr\" class=\"gmail_signature\"><div dir=\"ltr\">Anna Schmidt<br>Beispiel GmbH</div></div></div></blockquote></div>";
    let text = crate::mail::html_to_text(gmail);
    assert_eq!(text, "Sounds good, see you Friday.\n\n--\nHans Muster\nMuster AG\n\nOn Thu, Nov 12, 2026 at 9:15 AM Anna Schmidt <anna@example.org> wrote:\n\n> Can we meet on Friday?\n>\n> --\n> Anna Schmidt\n> Beispiel GmbH");
    assert_eq!(seg_shapes(&text), vec![(0, 0), (1, 1)], "the own text and the quote with its header; the signature is part of the segment before the quote");
}

// spec: IMP-064
#[test]
fn the_delimiter_ends_a_signature_at_the_end_of_its_segment_not_at_the_end_of_the_mail() {
    let segs = split(THUNDERBIRD);
    let starts: Vec<Option<usize>> = segs.iter().map(|s| signature_start(s, true)).collect();
    assert_eq!(starts[0], None);
    assert_eq!(starts[1], Some(6), "`> -- ` of the first quote, not the line of the older mail below it");
    assert_eq!(starts[2], Some(2), "`>> -- ` of the older mail");
    assert!(starts[3].is_some(), "the own signature at the end");
    let out = cut(THUNDERBIRD, SIGNATURES);
    assert!(!out.contains("-- "), "{out}");
    for kept in ["Hallo Anna,", "On 12.11.2026 09:15, Bob Meier wrote:", "> can we meet on Friday?", "> Bob\n", "> On 11.11.2026 17:00, Hans wrote:", ">> I sent the plan.", "Friday is fine."] {
        assert!(out.contains(kept), "{kept:?} stays: {out}");
    }
    for gone in ["Bob Meier\n", "Acme GmbH", "Tel +49", "Hans Muster", "Muster AG"] {
        assert!(!out.contains(gone), "{gone:?} goes: {out}");
    }
    assert_eq!(cut(THUNDERBIRD, TextCut::default()), THUNDERBIRD, "both options off: the text as it was");
}

// spec: IMP-064
#[test]
fn closing_greetings_and_a_disclaimer_find_the_signatures_of_an_outlook_thread() {
    let out = cut(OUTLOOK, SIGNATURES);
    assert!(out.contains("Mit freundlichen Grüßen\nHans Muster\n"), "the greeting and the name stay: {out}");
    assert!(out.contains("Viele Grüße\nAnna Schmidt\n"), "{out}");
    assert!(out.contains("Best regards,\nHans\n"), "{out}");
    for gone in ["Projektleiter", "Tel.: +49", "www.muster.example", "Beispiel GmbH", "Hauptstr. 1", "anna@example.org\n\n", "Mobil:", "may contain confidential"] {
        assert!(!out.contains(gone), "{gone:?} goes: {out}");
    }
    for kept in ["der Termin passt mir.", "Von: Anna Schmidt <anna@example.org>", "passt Freitag?", "-----Original Message-----", "From: Hans Muster", "When do you have time?"] {
        assert!(out.contains(kept), "{kept:?} stays: {out}");
    }
}

// spec: IMP-064
#[test]
fn a_german_mail_without_a_delimiter_loses_its_signature_block_after_the_greeting() {
    let out = cut(GERMAN, SIGNATURES);
    assert_eq!(out, "Hallo Frau Schmidt,\n\nanbei die Unterlagen für Freitag. Bitte melden Sie sich, falls etwas fehlt.\n\nViele Grüße\nHans Muster\n\n");
}

// spec: IMP-064
#[test]
fn a_disclaimer_alone_is_cut_and_a_name_after_the_greeting_alone_is_not() {
    let t = "Danke, ich melde mich.\n\nDiese E-Mail enthält vertrauliche Informationen. Wenn Sie nicht der richtige Adressat sind, informieren Sie bitte den Absender.\n";
    assert_eq!(cut(t, SIGNATURES), "Danke, ich melde mich.\n\n");
    let t = "Danke, ich melde mich.\n\nViele Grüße\nHans\n";
    assert_eq!(cut(t, SIGNATURES), t, "a name is no signature block");
    let t = "Danke.\n\nGrüße\nHans\nSenior Developer\n";
    assert_eq!(cut(t, SIGNATURES), t, "no way to reach the sender: not sure, so it stays");
}

// spec: IMP-064
#[test]
fn nothing_is_cut_in_a_code_block_a_mid_text_greeting_or_a_too_long_tail() {
    assert_eq!(cut(CODE, SIGNATURES), CODE, "a `-- ` in a fenced code block is no delimiter; a greeting with long lines after it is text");
    let long: String = format!("Hi\n\n-- \n{}", "line\n".repeat(60));
    assert_eq!(cut(&long, SIGNATURES), long, "a delimiter with a long text after it is not sure");
    assert_eq!(cut("a -- b\n--- \nc\n", SIGNATURES), "a -- b\n--- \nc\n", "only the delimiter line itself");
}

// spec: IMP-064
#[test]
fn with_answers_between_the_quoted_lines_only_a_delimiter_ends_the_text() {
    assert_eq!(cut(INLINE, SIGNATURES), INLINE, "the closing lines without a delimiter after a quote stay");
    let with = format!("{INLINE}\n-- \nHans\n").replace("\nBest regards\nHans\nMuster AG\n+49 30 1234567\n", "");
    let out = cut(&with, SIGNATURES);
    assert!(out.contains("It is in the second sheet.") && !out.contains("Hans"), "{out}");
    assert!(out.contains("> And the budget?") && out.contains("Yes, attached."));
}

// spec: IMP-064
#[test]
fn a_signature_that_stands_before_the_quote_ends_there() {
    let t = "Passt.\n\n-- \nHans\nMuster AG\n\nOn Thu, Nov 12, 2026 at 9:15 AM Anna wrote:\n> Geht Freitag?\n> Anna\n";
    assert_eq!(cut(t, SIGNATURES), "Passt.\n\n\nOn Thu, Nov 12, 2026 at 9:15 AM Anna wrote:\n> Geht Freitag?\n> Anna\n", "the quote below the signature stays");
}

// spec: IMP-064
#[test]
fn the_html_markers_of_the_four_clients_find_the_signature_before_the_quote() {
    let cases = [
        ("thunderbird", "<p>Answer</p><div class=\"moz-signature\">Hans Muster<br>Muster AG</div><div class=\"moz-cite-prefix\">On 12.11.26 09:15, Bob wrote:<br></div><blockquote type=\"cite\"><p>Question?</p></blockquote>"),
        ("gmail", "<div>Answer</div><div class=\"gmail_signature\"><div>Hans Muster<br>Muster AG</div></div><div class=\"gmail_quote\">On Thu, Nov 12, 2026 at 9:15 AM Bob wrote:<br><blockquote class=\"gmail_quote\">Question?</blockquote></div>"),
        ("outlook", "<div>Answer</div><div id=\"Signature\"><p>Hans Muster<br>Muster AG</p></div><div style=\"border-top:solid #E1E1E1 1.0pt\"><p><b>From:</b> Bob<br><b>Sent:</b> Thursday<br><b>To:</b> Hans<br><b>Subject:</b> Plan</p></div><p>Question?</p>"),
        ("apple", "<div>Answer</div><div id=\"AppleMailSignature\" dir=\"ltr\">Hans Muster<br>Muster AG</div><div><br>On 12 Nov 2026, at 09:15, Bob wrote:<br><blockquote type=\"cite\">Question?</blockquote></div>"),
    ];
    for (client, html) in cases {
        let text = crate::mail::html_to_text(html);
        assert!(text.contains("--\nHans Muster\nMuster AG\n"), "{client}: the marker makes a delimiter line: {text}");
        let out = cut(&text, SIGNATURES);
        assert!(out.contains("Answer") && out.contains("Question?") && !out.contains("Hans Muster") && !out.contains("Muster AG"), "{client}: {out}");
    }
    let outlook = crate::mail::html_to_text(cases[2].1);
    assert_eq!(seg_shapes(&outlook), vec![(0, 0), (1, 4)], "the From / Sent / To / Subject block of Outlook starts the quote");
}

// spec: IMP-064
#[test]
fn a_signature_container_that_has_its_delimiter_gets_no_second_one() {
    let t = crate::mail::html_to_text("<p>Hi</p><div class=\"moz-signature\">-- <br>Jane</div>");
    assert_eq!(t, "Hi\n\n--\nJane");
    let t = crate::mail::html_to_text("<p>Hi</p><p>Hello again</p>");
    assert_eq!(t, "Hi\n\nHello again", "no marker, no delimiter");
}

// spec: IMP-065
#[test]
fn skip_signatures_removes_every_signature_but_keeps_the_quoted_text_and_the_reply_headers() {
    let out = cut(OUTLOOK, SIGNATURES);
    let tidy = super::mail_text::tidy(&out);
    assert!(tidy.starts_with("Hallo Frau Schmidt,\n\nder Termin passt mir.\n\nMit freundlichen Grüßen\nHans Muster\n\n________________________________\nVon:"), "{tidy}");
    assert!(tidy.ends_with("When do you have time?\n\nBest regards,\nHans"), "{tidy}");
    let t = cut(THUNDERBIRD, SIGNATURES);
    assert!(t.contains("> Bob\n> On 11.11.2026 17:00, Hans wrote:\n>> I sent the plan.\n\nFriday is fine.\n"), "{t}");
}

// spec: IMP-066
#[test]
fn leave_out_quoted_keeps_the_own_text_and_drops_every_quote_with_its_header() {
    assert_eq!(cut(THUNDERBIRD, QUOTES), "Hallo Anna,\n\nFriday is fine.\n\n-- \nHans Muster\nMuster AG, Hauptstr. 1, 10115 Berlin\n");
    let out = cut(OUTLOOK, QUOTES);
    assert!(out.contains("Hallo Frau Schmidt") && out.contains("Mit freundlichen Grüßen") && !out.contains("Von:") && !out.contains("passt Freitag?") && !out.contains("Original Message"), "{out}");
    let out = cut(INLINE, QUOTES);
    assert!(out.starts_with("Yes, attached.\nIt is in the second sheet.") && !out.contains('>') && !out.contains("wrote:"), "the answers between the quotes stay: {out}");
    let both = cut(THUNDERBIRD, BOTH);
    assert_eq!(both, "Hallo Anna,\n\nFriday is fine.\n\n");
    let only_quote = "On Mon, Nov 9, 2026 at 10:00 AM Anna wrote:\n> Can you send the plan?\n";
    assert_eq!(cut(only_quote, QUOTES), only_quote, "when nothing else is left, the quote stays");
}

// spec: IMP-063, EDIT-083
#[test]
fn quoted_signatures_for_the_editor_end_at_the_quotes_segment() {
    let lines = vec!["> text", "> -- ", "> Anna", "> Acme", ">", "> > older", "> > -- ", "> > Bob", "x"];
    assert_eq!(quoted_signatures(&lines, |_| false), vec![(1, 5), (6, 8)]);
    assert_eq!(quoted_signatures(&lines, |l| l == 1), vec![(6, 8)], "code lines are skipped");
    assert_eq!(quoted_signatures(&["-- ", "Anna"], |_| false), vec![], "a delimiter outside a quote is EDIT-083's own");
}

proptest! {
    #![proptest_config(config())]

    // spec: IMP-063
    // The pieces mail text is made of: quote marks, headers, delimiters, greetings, code fences.
    #[test]
    fn imp_063_the_segments_joined_give_back_the_text(lines in prop::collection::vec(
        prop_oneof![
            3 => "[a-zA-Z ,.]{0,30}",
            2 => "(> ?){0,3}[a-z ]{0,12}",
            1 => Just("-- ".to_string()),
            1 => Just("On Mon, 1 Jan 2026 at 10:00, A wrote:".to_string()),
            1 => Just("Am 1.1.2026 schrieb A:".to_string()),
            1 => Just("-----Original Message-----".to_string()),
            1 => Just("From: a\r".to_string()),
            1 => Just("Sent: b".to_string()),
            1 => Just("Subject: c".to_string()),
            1 => Just("To: d".to_string()),
            1 => Just("Viele Grüße".to_string()),
            1 => Just("```".to_string()),
            1 => Just("tel: +49 30 123456".to_string()),
        ],
        0..14,
    ), end in prop_oneof![Just(""), Just("\n")]) {
        let text = format!("{}{end}", lines.join("\n"));
        let segs = split(&text);
        prop_assert_eq!(join(&segs), text.clone());
        prop_assert!(segs.iter().all(|s| !s.text.is_empty() && s.header_lines <= s.text.lines().count()));
        // what is cut out is whole lines: the rest is the text with some lines gone, in order
        for c in [SIGNATURES, QUOTES, BOTH] {
            let out = cut(&text, c);
            let mut rest = text.split_inclusive('\n');
            for l in out.split_inclusive('\n') {
                prop_assert!(rest.any(|r| r == l), "{l:?} is not a line of the text, in order");
            }
        }
        prop_assert_eq!(cut(&text, TextCut::default()), text);
    }
}
