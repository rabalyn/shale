//! Tests of what the Word import reads from the file itself (IMP-040, IMP-041): the look from the
//! XML parts, the tagging of coloured runs, the Markdown that comes back and the zip of the
//! tagged copy. None of them needs pandoc.

use super::docx_look::*;
use std::path::PathBuf;

const NS: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;

fn doc(body: &str) -> String {
    format!(r#"<w:document {NS}><w:body>{body}</w:body></w:document>"#)
}

fn sect(w: u32, h: u32, orient: &str) -> String {
    format!(r#"<w:sectPr><w:pgSz w:w="{w}" w:h="{h}"{orient}/><w:pgMar w:top="1134" w:right="1417" w:bottom="1440" w:left="1701"/></w:sectPr>"#)
}

fn styles(defaults: &str, rest: &str) -> String {
    format!(r#"<w:styles {NS}><w:docDefaults>{defaults}</w:docDefaults>{rest}</w:styles>"#)
}

const THEME: &str = r#"<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:themeElements>
<a:clrScheme name="x"><a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1><a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1><a:dk2><a:srgbClr val="0E2841"/></a:dk2><a:accent1><a:srgbClr val="156082"/></a:accent1><a:accent2><a:srgbClr val="FF0000"/></a:accent2></a:clrScheme>
<a:fontScheme name="x"><a:majorFont><a:latin typeface="Georgia"/></a:majorFont><a:minorFont><a:latin typeface="Aptos"/></a:minorFont></a:fontScheme>
</a:themeElements></a:theme>"#;

fn props(l: &Look) -> Vec<String> {
    l.props().into_iter().map(|(k, v)| format!("{k}: {v}")).collect()
}

// spec: IMP-040
#[test]
fn a_full_look_gives_the_properties_in_the_order_of_the_contract() {
    let s = styles(
        r#"<w:rPrDefault><w:rPr><w:rFonts w:asciiTheme="minorHAnsi"/><w:sz w:val="22"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after="160" w:line="276" w:lineRule="auto"/></w:pPr></w:pPrDefault>"#,
        r#"<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:pPr><w:jc w:val="both"/></w:pPr></w:style>
<w:style w:type="paragraph" w:styleId="Heading1"><w:rPr><w:color w:val="2f5496" w:themeColor="accent1" w:themeShade="BF"/></w:rPr></w:style>"#,
    );
    let l = read(&doc(&sect(11906, 16838, "")), Some(&s), Some(THEME));
    assert_eq!(
        props(&l),
        [
            "print-paper: A4",
            "print-margins: 20 25 25.4 30",
            "print-font: sans",
            "print-font-size: 11",
            "print-line-spacing: 1.15",
            "print-paragraph-spacing: 8",
            "print-align: justify",
            "print-heading-color: \"#0F4761\"",
            "print-title: false",
            "print-toc: false",
        ]
    );
}

// spec: IMP-040
#[test]
fn twips_become_millimetres_with_one_decimal_and_no_trailing_zero() {
    let l = read(&doc(&sect(12240, 15840, "")), None, None);
    assert_eq!(props(&l)[..2], ["print-paper: Letter", "print-margins: 20 25 25.4 30"]);
    let l = read(&doc(r#"<w:sectPr><w:pgSz w:w="12000" w:h="15000"/><w:pgMar w:top="720" w:right="567" w:bottom="1" w:left="0"/></w:sectPr>"#), None, None);
    assert_eq!(props(&l)[..2], ["print-paper: 211.7x264.6mm", "print-margins: 12.7 10 0 0"]);
}

// spec: IMP-040
#[test]
fn paper_sizes_match_within_twenty_twips_in_either_orientation_and_landscape_is_wider_than_tall() {
    let paper = |w: u32, h: u32, o: &str| props(&read(&doc(&sect(w, h, o)), None, None))[0].clone();
    assert_eq!(paper(11906, 16838, ""), "print-paper: A4");
    assert_eq!(paper(11920, 16820, ""), "print-paper: A4", "within 20 twips");
    assert_eq!(paper(11930, 16838, ""), "print-paper: 210.4x297mm", "more than 20 twips off: no name");
    assert_eq!(paper(8391, 11906, ""), "print-paper: A5");
    assert_eq!(paper(12240, 20160, ""), "print-paper: Legal");
    assert_eq!(paper(16838, 11906, r#" w:orient="landscape""#), "print-paper: 297x210mm");
    assert_eq!(paper(16838, 11906, ""), "print-paper: 297x210mm", "a page wider than tall is landscape without the attribute");
    assert_eq!(paper(11906, 16838, r#" w:orient="landscape""#), "print-paper: 297x210mm", "the attribute with a portrait size: swapped");
    assert_eq!(paper(15840, 12240, ""), "print-paper: 279.4x215.9mm");
}

// spec: IMP-040
#[test]
fn the_last_section_holds_the_page_setup() {
    let first = r#"<w:p><w:pPr><w:sectPr><w:pgSz w:w="8391" w:h="11906"/></w:sectPr></w:pPr></w:p>"#;
    let l = read(&doc(&format!("{first}{}", sect(11906, 16838, ""))), None, None);
    assert_eq!(props(&l)[0], "print-paper: A4");
}

// spec: IMP-040
#[test]
fn fonts_are_sorted_into_serif_sans_and_mono_and_unknown_names_are_left_out() {
    for (name, family) in [
        ("Calibri", Some("sans")),
        ("Arial", Some("sans")),
        ("Aptos", Some("sans")),
        ("Segoe UI", Some("sans")),
        ("Open Sans", Some("sans")),
        ("Times New Roman", Some("serif")),
        ("Cambria", Some("serif")),
        ("Book Antiqua", Some("serif")),
        ("Liberation Serif", Some("serif")),
        ("Courier New", Some("mono")),
        ("Consolas", Some("mono")),
        ("DejaVu Sans Mono", Some("mono")),
        ("Wingdings Fantasy", None),
    ] {
        assert_eq!(font_family(name), family, "{name}");
    }
    let named = |name: &str| styles(&format!(r#"<w:rPrDefault><w:rPr><w:rFonts w:ascii="{name}" w:hAnsi="{name}"/></w:rPr></w:rPrDefault>"#), "");
    let l = read(&doc(""), Some(&named("Zapfino")), None);
    assert!(!props(&l).iter().any(|p| p.starts_with("print-font:")), "{:?}", props(&l));
}

// spec: IMP-040
#[test]
fn theme_fonts_are_resolved_and_a_theme_font_wins_over_a_name() {
    let s = |rfonts: &str, normal: &str| styles(&format!(r#"<w:rPrDefault><w:rPr>{rfonts}</w:rPr></w:rPrDefault>"#), normal);
    let font = |s: &str| props(&read(&doc(""), Some(s), Some(THEME))).into_iter().find(|p| p.starts_with("print-font:"));
    assert_eq!(font(&s(r#"<w:rFonts w:asciiTheme="minorHAnsi"/>"#, "")).as_deref(), Some("print-font: sans"));
    assert_eq!(font(&s(r#"<w:rFonts w:asciiTheme="majorHAnsi"/>"#, "")).as_deref(), Some("print-font: serif"));
    assert_eq!(font(&s(r#"<w:rFonts w:hAnsiTheme="minorBidi"/>"#, "")).as_deref(), Some("print-font: sans"));
    assert_eq!(font(&s(r#"<w:rFonts w:ascii="Courier New" w:asciiTheme="minorHAnsi"/>"#, "")).as_deref(), Some("print-font: sans"));
    // Normal overrides the defaults
    let normal = r#"<w:style w:type="paragraph" w:styleId="Normal"><w:rPr><w:rFonts w:ascii="Courier New"/></w:rPr></w:style>"#;
    assert_eq!(font(&s(r#"<w:rFonts w:asciiTheme="minorHAnsi"/>"#, normal)).as_deref(), Some("print-font: mono"));
    // a theme font without a theme is unknown
    let no_theme = props(&read(&doc(""), Some(&s(r#"<w:rFonts w:asciiTheme="minorHAnsi"/>"#, "")), None));
    assert!(!no_theme.iter().any(|p| p.starts_with("print-font:")));
}

// spec: IMP-040
#[test]
fn line_spacing_is_a_factor_for_auto_and_a_ratio_to_the_font_size_for_exact_and_at_least() {
    let with = |spacing: &str| {
        let d = format!(r#"<w:rPrDefault><w:rPr><w:sz w:val="24"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr>{spacing}</w:pPr></w:pPrDefault>"#);
        props(&read(&doc(""), Some(&styles(&d, "")), None)).into_iter().filter(|p| p.starts_with("print-line") || p.starts_with("print-para")).collect::<Vec<_>>()
    };
    assert_eq!(with(r#"<w:spacing w:line="360" w:lineRule="auto" w:after="120"/>"#), ["print-line-spacing: 1.5", "print-paragraph-spacing: 6"]);
    assert_eq!(with(r#"<w:spacing w:line="480"/>"#), ["print-line-spacing: 2"], "no rule means auto");
    assert_eq!(with(r#"<w:spacing w:line="300" w:lineRule="exact"/>"#), ["print-line-spacing: 1.25"], "15 pt on 12 pt");
    assert_eq!(with(r#"<w:spacing w:line="280" w:lineRule="atLeast" w:after="150"/>"#), ["print-line-spacing: 1.17", "print-paragraph-spacing: 7.5"], "14 pt on 12 pt, two decimals");
    assert!(with("").is_empty());
    // the exact value needs a font size
    let no_size = styles(r#"<w:pPrDefault><w:pPr><w:spacing w:line="300" w:lineRule="exact"/></w:pPr></w:pPrDefault>"#, "");
    assert!(!props(&read(&doc(""), Some(&no_size), None)).iter().any(|p| p.starts_with("print-line")));
}

// spec: IMP-040
#[test]
fn the_size_and_alignment_come_from_normal_over_the_defaults_and_only_both_and_distribute_justify() {
    let s = |jc: &str| styles(r#"<w:rPrDefault><w:rPr><w:sz w:val="21"/></w:rPr></w:rPrDefault>"#, &format!(r#"<w:style w:type="paragraph" w:styleId="Normal"><w:pPr><w:jc w:val="{jc}"/></w:pPr></w:style>"#));
    let get = |jc: &str| props(&read(&doc(""), Some(&s(jc)), None));
    assert!(get("both").contains(&"print-font-size: 10.5".to_string()));
    assert!(get("both").contains(&"print-align: justify".to_string()));
    assert!(get("distribute").contains(&"print-align: justify".to_string()));
    for jc in ["left", "start", "center", "right"] {
        assert!(get(jc).contains(&"print-align: left".to_string()), "{jc}");
    }
    let over = styles(r#"<w:rPrDefault><w:rPr><w:sz w:val="21"/></w:rPr></w:rPrDefault>"#, r#"<w:style w:type="paragraph" w:styleId="Normal"><w:rPr><w:sz w:val="24"/></w:rPr></w:style>"#);
    assert!(props(&read(&doc(""), Some(&over), None)).contains(&"print-font-size: 12".to_string()));
}

// spec: IMP-040
#[test]
fn the_heading_colour_is_the_value_or_the_theme_colour_and_black_or_auto_is_left_out() {
    let h1 = |color: &str| styles("", &format!(r#"<w:style w:type="paragraph" w:styleId="Heading1"><w:rPr>{color}</w:rPr></w:style>"#));
    let colour = |s: &str, theme: Option<&str>| props(&read(&doc(""), Some(s), theme)).into_iter().find(|p| p.starts_with("print-heading-color"));
    assert_eq!(colour(&h1(r#"<w:color w:val="c00000"/>"#), None).as_deref(), Some("print-heading-color: \"#C00000\""));
    assert_eq!(colour(&h1(r#"<w:color w:val="auto" w:themeColor="accent1"/>"#), Some(THEME)).as_deref(), Some("print-heading-color: \"#156082\""));
    assert_eq!(colour(&h1(r#"<w:color w:themeColor="accent1"/>"#), Some(THEME)).as_deref(), Some("print-heading-color: \"#156082\""));
    assert_eq!(colour(&h1(r#"<w:color w:val="C00000" w:themeColor="accent1"/>"#), Some(THEME)).as_deref(), Some("print-heading-color: \"#156082\""), "the theme colour wins over the value");
    assert_eq!(colour(&h1(r#"<w:color w:val="C00000" w:themeColor="accent1"/>"#), None).as_deref(), Some("print-heading-color: \"#C00000\""), "no theme: the value");
    assert_eq!(colour(&h1(r#"<w:color w:themeColor="accent1" w:themeShade="BF"/>"#), Some(THEME)).as_deref(), Some("print-heading-color: \"#0F4761\""), "shade");
    assert_eq!(colour(&h1(r#"<w:color w:themeColor="text1"/>"#), Some(THEME)), None, "black");
    assert_eq!(colour(&h1(r#"<w:color w:val="000000"/>"#), None), None);
    assert_eq!(colour(&h1(r#"<w:color w:val="auto"/>"#), None), None);
    assert_eq!(colour(&h1(r#"<w:color w:themeColor="accent1"/>"#), None), None, "no theme to resolve it");
    assert_eq!(colour(&h1(""), None), None);
    assert_eq!(colour(&styles("", ""), None), None, "no Heading1");
}

// spec: IMP-040
#[test]
fn missing_parts_leave_their_keys_out_and_a_broken_file_gives_nothing() {
    // no styles, no theme, no section: only what the document itself says
    assert_eq!(props(&read(&doc(""), None, None)), ["print-title: false", "print-toc: false"]);
    // styles that do not parse count as missing
    assert_eq!(props(&read(&doc(&sect(11906, 16838, "")), Some("<w:styles"), Some("nope"))).len(), 4);
    // an empty styles part: the body alignment is left
    assert!(props(&read(&doc(""), Some(&styles("", "")), None)).contains(&"print-align: left".to_string()));
    // a section without sizes
    assert_eq!(props(&read(&doc("<w:sectPr/>"), None, None)), ["print-title: false", "print-toc: false"]);
    assert_eq!(props(&read(&doc(r#"<w:sectPr><w:pgMar w:top="1" w:right="1"/></w:sectPr>"#), None, None)), ["print-title: false", "print-toc: false"], "margins need all four sides");
    // no document: nothing at all
    assert!(read("not xml", Some(&styles("", "")), None).props().is_empty());
    assert!(Look::default().props().is_empty());
}

// spec: IMP-040
#[test]
fn a_table_of_contents_is_found_by_its_field_or_its_content_control_and_then_print_toc_is_left_out() {
    let toc_field = doc(r#"<w:p><w:r><w:instrText xml:space="preserve"> TOC \o "1-3" \h \z \u </w:instrText></w:r></w:p>"#);
    let simple = doc(r#"<w:p><w:fldSimple w:instr=" TOC \o &quot;1-3&quot; "/></w:p>"#);
    let sdt = doc(r#"<w:sdt><w:sdtPr><w:docPartObj><w:docPartGallery w:val="Table of Contents"/></w:docPartObj></w:sdtPr></w:sdt>"#);
    for d in [toc_field, simple, sdt] {
        assert_eq!(props(&read(&d, None, None)), ["print-title: false"], "{d}");
    }
    let other = doc(r#"<w:p><w:r><w:instrText> PAGE </w:instrText></w:r><w:r><w:instrText> TOCX </w:instrText></w:r></w:p>"#);
    assert!(props(&read(&other, None, None)).contains(&"print-toc: false".to_string()));
}

/// `tag_runs` without styles and theme.
fn tag_one(document: &str) -> Option<String> {
    tag_runs(document, None, None)
}

fn run(rpr: &str, text: &str) -> String {
    format!(r#"<w:r><w:rPr>{rpr}</w:rPr><w:t xml:space="preserve">{text}</w:t></w:r>"#)
}

// spec: IMP-041
#[test]
fn a_coloured_run_gets_sentinels_around_its_text_with_the_white_space_outside() {
    let d = doc(&format!("<w:p>{}</w:p>", run(r#"<w:b/><w:color w:val="c00000"/>"#, "  red &amp; text ")));
    let tagged = tag_one(&d).unwrap();
    assert!(tagged.contains("<w:t xml:space=\"preserve\">  \u{E000}c:C00000\u{E001}red &amp; text\u{E002} </w:t>"), "{tagged}");
    // everything else is as it was
    assert_eq!(tagged.replace(['\u{E000}', '\u{E001}', '\u{E002}'], "").replace("c:C00000", ""), d);
}

// spec: IMP-041
#[test]
fn runs_without_a_colour_or_with_auto_black_or_no_text_are_left_alone() {
    for rpr in [r#"<w:b/>"#, r#"<w:color w:val="auto"/>"#, r#"<w:color w:val="000000"/>"#, r#"<w:color w:val="zzzzzz"/>"#, r#"<w:highlight w:val="yellow"/>"#, r#"<w:highlight w:val="none"/>"#] {
        assert_eq!(tag_one(&doc(&format!("<w:p>{}</w:p>", run(rpr, "x")))), None, "{rpr}");
    }
    assert_eq!(tag_one(&doc("<w:p><w:r><w:t>no properties</w:t></w:r></w:p>")), None);
    assert_eq!(tag_one(&doc(&format!("<w:p>{}</w:p>", run(r#"<w:color w:val="FF0000"/>"#, "   ")))), None, "only white space");
    assert_eq!(tag_one("not xml"), None);
}

// spec: IMP-041
#[test]
fn a_highlight_other_than_yellow_is_tagged_and_its_element_taken_out_but_yellow_stays_for_pandoc() {
    let d = doc(&format!("<w:p>{}</w:p>", run(r#"<w:highlight w:val="darkGreen"/>"#, "marked")));
    let tagged = tag_one(&d).unwrap();
    assert!(tagged.contains("\u{E000}h:008000\u{E001}marked\u{E003}") && !tagged.contains("w:highlight"), "{tagged}");
    // a colour and a highlight: the colour outside
    let both = tag_one(&doc(&format!("<w:p>{}</w:p>", run(r#"<w:color w:val="0000FF"/><w:highlight w:val="red"/>"#, "x")))).unwrap();
    assert!(both.contains("\u{E000}c:0000FF\u{E001}\u{E000}h:FF0000\u{E001}x\u{E003}\u{E002}") && !both.contains("w:highlight"), "{both}");
    // yellow: the run keeps its highlight, but its colour is tagged
    let yellow = tag_one(&doc(&format!("<w:p>{}</w:p>", run(r#"<w:color w:val="0000FF"/><w:highlight w:val="yellow"/>"#, "x")))).unwrap();
    assert!(yellow.contains(r#"<w:highlight w:val="yellow"/>"#) && !yellow.contains("h:"), "{yellow}");
    // shading is a highlight colour
    let shaded = tag_one(&doc(&format!("<w:p>{}</w:p>", run(r#"<w:shd w:val="clear" w:color="auto" w:fill="ffc000"/>"#, "x")))).unwrap();
    assert!(shaded.contains("\u{E000}h:FFC000\u{E001}x\u{E003}"), "{shaded}");
    let auto = r#"<w:shd w:val="clear" w:color="auto" w:fill="auto"/>"#;
    assert_eq!(tag_one(&doc(&format!("<w:p>{}</w:p>", run(auto, "x")))), None);
}

// spec: IMP-041
#[test]
fn every_text_of_a_run_is_tagged_and_several_runs_each_get_their_own() {
    let r = r#"<w:r><w:rPr><w:color w:val="112233"/></w:rPr><w:t>a</w:t><w:tab/><w:t>b</w:t></w:r>"#;
    let tagged = tag_one(&doc(&format!("<w:p>{r}{r}</w:p>"))).unwrap();
    assert_eq!(tagged.matches('\u{E000}').count(), 4);
    assert_eq!(tagged.matches('\u{E002}').count(), 4);
    let parsed = roxmltree::Document::parse(&tagged);
    assert!(parsed.is_ok(), "the tagged text is still XML");
}

// spec: IMP-041
#[test]
fn sentinels_become_spans_and_marks_in_the_canonical_form() {
    let text = "a \u{E000}c:C00000\u{E001}red\u{E002} b \u{E000}h:00FF00\u{E001}green\u{E003} c \u{E000}c:0000FF\u{E001}\u{E000}h:FF0000\u{E001}both\u{E003}\u{E002}.";
    assert_eq!(colours_to_html(text), "a <span style=\"color: #C00000;\">red</span> b <mark style=\"background: #00FF00;\">green</mark> c <span style=\"color: #0000FF;\"><mark style=\"background: #FF0000;\">both</mark></span>.");
    assert_eq!(colours_to_html("plain **text**"), "plain **text**");
    // emphasis outside: runs next to each other with the same colour are one span
    let two = "\u{E000}c:C00000\u{E001}one\u{E002}\u{E000}c:C00000\u{E001} two\u{E002}\u{E000}c:00B050\u{E001} three\u{E002}";
    assert_eq!(colours_to_html(two), "<span style=\"color: #C00000;\">one two</span><span style=\"color: #00B050;\"> three</span>");
}

// spec: IMP-041
#[test]
fn stray_sentinels_are_dropped_and_what_is_left_open_is_closed() {
    assert_eq!(colours_to_html("a\u{E002}b\u{E003}c"), "abc", "a closing without an opening");
    assert_eq!(colours_to_html("a \u{E000}c:12\u{E001}b \u{E000}x:123456\u{E001}c \u{E000}"), "a c:12b x:123456c ", "the sentinels of what is not a tag go, its text stays");
    assert_eq!(colours_to_html("\u{E000}c:112233\u{E001}open"), "<span style=\"color: #112233;\">open</span>");
    assert_eq!(colours_to_html("\u{E000}h:112233\u{E001}x\u{E002}y"), "<mark style=\"background: #112233;\">xy</mark>", "a closing of the other kind is dropped");
    assert!(!colours_to_html("\u{E000}\u{E001}\u{E002}\u{E003}").contains(['\u{E000}', '\u{E001}', '\u{E002}', '\u{E003}']));
}

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("shale-docx-look-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

// spec: IMP-041
#[test]
fn the_zip_of_the_tagged_copy_reads_back_with_every_file_and_the_content_types_first() {
    let src = scratch("zip-src");
    let files: [(&str, &[u8]); 4] = [("[Content_Types].xml", b"<Types/>"), ("word/document.xml", "<w:document>Übung</w:document>".as_bytes()), ("word/media/image1.png", &[0, 1, 2, 255]), ("_rels/.rels", b"")];
    for (name, data) in files {
        let p = src.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, data).unwrap();
    }
    let zip = scratch("zip-out").join("t.docx");
    write_zip(&src, &zip).unwrap();
    let bytes = std::fs::read(&zip).unwrap();
    assert_eq!(&bytes[..4], b"PK\x03\x04");
    assert_eq!(&bytes[30..48], b"[Content_Types].xm", "the content types are the first entry");
    let back = scratch("zip-back");
    crate::pdf::tex::archive::extract(&bytes, &back).unwrap();
    for (name, data) in files {
        assert_eq!(std::fs::read(back.join(name)).unwrap(), data, "{name}");
    }
    // the same files give the same bytes
    let again = scratch("zip-out2").join("t.docx");
    write_zip(&src, &again).unwrap();
    assert_eq!(std::fs::read(&again).unwrap(), bytes);
}

// spec: IMP-040, IMP-041
#[test]
fn a_file_that_is_no_zip_gives_no_look_and_no_copy_and_a_real_one_gives_both() {
    let work = scratch("analyse");
    let bad = work.join("bad.docx");
    std::fs::write(&bad, b"PK fake docx").unwrap();
    let a = analyse(&bad, &work.join("w1"));
    assert!(a.look.props().is_empty() && a.tagged.is_none());
    let a = analyse(&work.join("missing.docx"), &work.join("w2"));
    assert!(a.look.props().is_empty() && a.tagged.is_none());
    // a small file of our own: the document with a coloured run and a page
    let parts = scratch("analyse-parts");
    let body = format!("<w:p>{}</w:p>", run(r#"<w:color w:val="C00000"/>"#, "red"));
    for (name, text) in [("[Content_Types].xml", "<Types/>".to_string()), ("word/document.xml", doc(&format!("{body}{}", sect(12240, 15840, ""))))] {
        let p = parts.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    let docx = work.join("ok.docx");
    write_zip(&parts, &docx).unwrap();
    let before = std::fs::read(&docx).unwrap();
    let a = analyse(&docx, &work.join("w3"));
    assert_eq!(props(&a.look)[0], "print-paper: Letter");
    let tagged = a.tagged.expect("a coloured run gives a copy");
    let back = scratch("analyse-back");
    crate::pdf::tex::archive::extract(&std::fs::read(&tagged).unwrap(), &back).unwrap();
    assert!(std::fs::read_to_string(back.join("word/document.xml")).unwrap().contains("\u{E000}c:C00000\u{E001}red\u{E002}"));
    assert_eq!(std::fs::read(&docx).unwrap(), before, "the original is untouched");
}

const RED: &str = r#"<w:color w:val="C00000"/>"#;

fn style(kind: &str, id: &str, name: &str, based: &str, rpr: &str) -> String {
    let based = if based.is_empty() { String::new() } else { format!(r#"<w:basedOn w:val="{based}"/>"#) };
    format!(r#"<w:style w:type="{kind}" w:styleId="{id}"><w:name w:val="{name}"/>{based}<w:rPr>{rpr}</w:rPr></w:style>"#)
}

fn styled_run(rstyle: &str, text: &str) -> String {
    format!(r#"<w:r><w:rPr><w:rStyle w:val="{rstyle}"/></w:rPr><w:t>{text}</w:t></w:r>"#)
}

fn para(pstyle: &str, runs: &str) -> String {
    format!(r#"<w:p><w:pPr><w:pStyle w:val="{pstyle}"/></w:pPr>{runs}</w:p>"#)
}

/// The text of `x` runs that came out tagged, as `RRGGBB:text`.
fn tagged_colours(d: &str, s: &str, theme: Option<&str>) -> Vec<String> {
    let Some(t) = tag_runs(d, Some(s), theme) else { return Vec::new() };
    let mut out = Vec::new();
    let mut rest = t.as_str();
    while let Some(at) = rest.find("\u{E000}c:") {
        let tail = &rest[at + 5..];
        let end = tail.find('\u{E002}').unwrap();
        out.push(tail[..end].replace('\u{E001}', ":"));
        rest = &tail[end..];
    }
    out
}

// spec: IMP-041
#[test]
fn a_character_style_colours_its_runs_also_through_based_on_and_direct_colour_wins() {
    let s = styles(
        "",
        &[
            style("character", "Red", "Red", "", RED),
            style("character", "RedToo", "Red too", "Red", "<w:b/>"),
            style("character", "Blue", "Blue", "Red", r#"<w:color w:val="0000FF"/>"#),
            style("character", "Loop1", "Loop 1", "Loop2", ""),
            style("character", "Loop2", "Loop 2", "Loop1", ""),
        ]
        .concat(),
    );
    let runs = format!(
        "{}{}{}{}{}",
        styled_run("Red", "a"),
        styled_run("RedToo", "b"),
        styled_run("Blue", "c"),
        styled_run("Loop1", "d"),
        r#"<w:r><w:rPr><w:rStyle w:val="Red"/><w:color w:val="00B050"/></w:rPr><w:t>e</w:t></w:r><w:r><w:rPr><w:rStyle w:val="Red"/><w:color w:val="auto"/></w:rPr><w:t>f</w:t></w:r>"#
    );
    assert_eq!(tagged_colours(&doc(&format!("<w:p>{runs}</w:p>")), &s, None), ["C00000:a", "C00000:b", "0000FF:c", "00B050:e"]);
}

// spec: IMP-041
#[test]
fn a_paragraph_style_colours_the_runs_below_a_character_style_and_a_table_style_below_both() {
    let s = styles(
        "",
        &[
            style("paragraph", "Note", "Note", "", RED),
            style("paragraph", "SubNote", "Sub note", "Note", ""),
            style("character", "Blue", "Blue", "", r#"<w:color w:val="0000FF"/>"#),
            style("table", "Grid", "Grid", "", r#"<w:color w:val="7030A0"/>"#),
        ]
        .concat(),
    );
    let plain = r#"<w:r><w:t>x</w:t></w:r>"#;
    let d = doc(&format!("{}{}<w:p>{}</w:p>", para("SubNote", plain), para("Note", &styled_run("Blue", "y")), plain));
    assert_eq!(tagged_colours(&d, &s, None), ["C00000:x", "0000FF:y"]);
    let table = format!(r#"<w:tbl><w:tblPr><w:tblStyle w:val="Grid"/></w:tblPr><w:tr><w:tc>{}{}</w:tc></w:tr></w:tbl>"#, para("Normal", plain), format_args!("<w:p>{plain}</w:p>"));
    // an explicit paragraph style (even Normal, which sets none) leaves the table style in; the second paragraph has none
    assert_eq!(tagged_colours(&doc(&table), &s, None), ["7030A0:x", "7030A0:x"]);
    let in_note = format!(r#"<w:tbl><w:tblPr><w:tblStyle w:val="Grid"/></w:tblPr><w:tr><w:tc>{}</w:tc></w:tr></w:tbl>"#, para("Note", plain));
    assert_eq!(tagged_colours(&doc(&in_note), &s, None), ["C00000:x"], "the paragraph style is above the table style");
}

// spec: IMP-041
#[test]
fn the_colour_of_the_body_needs_no_span_and_a_style_can_set_black_again() {
    let defaults = r#"<w:rPrDefault><w:rPr><w:color w:val="404040"/></w:rPr></w:rPrDefault>"#;
    let s = styles(
        defaults,
        &[
            style("paragraph", "Normal", "Normal", "", ""),
            style("character", "Grey", "Grey", "", r#"<w:color w:val="404040"/>"#),
            style("character", "Black", "Black", "Grey", r#"<w:color w:val="auto"/>"#),
            style("character", "Red", "Red", "", RED),
        ]
        .concat(),
    );
    let runs = format!("{}{}{}{}", styled_run("Grey", "a"), styled_run("Black", "b"), styled_run("Red", "c"), r#"<w:r><w:rPr><w:color w:val="404040"/></w:rPr><w:t>d</w:t></w:r>"#);
    assert_eq!(tagged_colours(&doc(&format!("<w:p>{runs}</w:p>")), &s, None), ["C00000:c"]);
    // Normal over the defaults
    let s = styles(defaults, &style("paragraph", "Normal", "Normal", "", RED));
    let both = format!("<w:p>{}{}</w:p>", styled_run("Normal", "a"), r#"<w:r><w:rPr><w:color w:val="404040"/></w:rPr><w:t>b</w:t></w:r>"#);
    assert_eq!(tagged_colours(&doc(&both), &s, None), ["404040:b"], "the body is red now: red needs no span, grey does");
}

// spec: IMP-041
#[test]
fn heading_styles_and_hyperlink_give_no_span_but_a_colour_of_its_own_in_a_heading_does() {
    let s = styles(
        "",
        &[
            style("paragraph", "Heading1", "heading 1", "", RED),
            style("paragraph", "Heading2", "heading 2", "", RED),
            style("paragraph", "Title", "Title", "", RED),
            style("paragraph", "MyHead", "My head", "Heading1", ""),
            style("paragraph", "Odd", "Heading 3", "", RED),
            style("character", "Hyperlink", "Hyperlink", "", r#"<w:color w:val="0563C1"/>"#),
        ]
        .concat(),
    );
    let plain = r#"<w:r><w:t>x</w:t></w:r>"#;
    let own = |c: &str| format!(r#"<w:r><w:rPr><w:color w:val="{c}"/></w:rPr><w:t>y</w:t></w:r>"#);
    let d = doc(&format!("{}{}{}{}{}{}", para("Heading1", plain), para("Heading2", plain), para("Title", plain), para("MyHead", plain), para("Odd", plain), para("Normal", &styled_run("Hyperlink", "link"))));
    assert_eq!(tagged_colours(&d, &s, None), Vec::<String>::new());
    // in a heading the heading's own colour needs no span, another one does
    assert_eq!(tagged_colours(&doc(&format!("{}{}", para("Heading1", &own("C00000")), para("Heading1", &own("00B050")))), &s, None), ["00B050:y"]);
}

// spec: IMP-041
#[test]
fn theme_colours_are_resolved_for_runs_and_the_theme_colour_wins_over_the_value() {
    let r = |color: &str| format!(r#"<w:r><w:rPr>{color}</w:rPr><w:t>x</w:t></w:r>"#);
    let colour = |c: &str, theme: Option<&str>| tagged_colours(&doc(&format!("<w:p>{}</w:p>", r(c))), &styles("", ""), theme);
    assert_eq!(colour(r#"<w:color w:themeColor="accent1"/>"#, Some(THEME)), ["156082:x"]);
    assert_eq!(colour(r#"<w:color w:val="auto" w:themeColor="dk2"/>"#, Some(THEME)), ["0E2841:x"]);
    assert_eq!(colour(r#"<w:color w:val="C00000" w:themeColor="accent1"/>"#, Some(THEME)), ["156082:x"]);
    assert_eq!(colour(r#"<w:color w:val="C00000" w:themeColor="accent1"/>"#, None), ["C00000:x"], "no theme: the value");
    assert_eq!(colour(r#"<w:color w:themeColor="hlink"/>"#, Some(THEME)), Vec::<String>::new(), "a slot the theme lacks");
    assert_eq!(colour(r#"<w:color w:themeColor="text1"/>"#, Some(THEME)), Vec::<String>::new(), "black");
    // the names of Word
    assert_eq!(colour(r#"<w:color w:themeColor="background1" w:themeShade="80"/>"#, Some(THEME)), ["808080:x"]);
    assert_eq!(colour(r#"<w:color w:themeColor="text2"/>"#, Some(THEME)), ["0E2841:x"]);
}

// spec: IMP-041
#[test]
fn tint_lifts_the_lightness_towards_white_and_shade_pulls_it_towards_black() {
    let r = |color: &str| {
        let d = doc(&format!(r#"<w:p><w:r><w:rPr>{color}</w:rPr><w:t>x</w:t></w:r></w:p>"#));
        tagged_colours(&d, &styles("", ""), Some(THEME)).concat()
    };
    // Word's own value: accent1 with shade BF is the colour of Heading 1 in its new default template
    assert_eq!(r(r#"<w:color w:themeColor="accent1" w:themeShade="BF"/>"#), "0F4761:x");
    // 0x80 / 255 of the lightness: red (L 0.5) gets L 0.749, white 0.502
    assert_eq!(r(r#"<w:color w:themeColor="accent2" w:themeTint="80"/>"#), "FF7F7F:x");
    assert_eq!(r(r#"<w:color w:themeColor="background1" w:themeShade="80"/>"#), "808080:x");
    assert_eq!(r(r#"<w:color w:themeColor="accent2" w:themeShade="80"/>"#), "800000:x");
    assert_eq!(r(r#"<w:color w:themeColor="accent2" w:themeTint="FF"/>"#), "FF0000:x", "tint FF keeps the colour");
    assert_eq!(r(r#"<w:color w:themeColor="accent2" w:themeTint="00"/>"#), "FFFFFF:x", "tint 00 is white");
}
