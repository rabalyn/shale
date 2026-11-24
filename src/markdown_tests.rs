//! Property tests of the Markdown parsing (LINK-018, DEV-042): whatever text a note holds, the
//! scanners do not panic and what they return lies inside the text. The example tests are in
//! `markdown.rs`.

use std::ops::Range;

use proptest::prelude::*;

use crate::markdown::*;
use crate::property_test_support::{config, config_with};

/// Text that looks like a note, made to reach the parsers' edge cases: pieces of markup (links,
/// code, math, tags, tasks, headings, footnotes, front matter, wrappers, page breaks), the three
/// line endings, long runs of markup characters, and non-ASCII, mixed with arbitrary text.
fn md_text() -> impl Strategy<Value = String> {
    let piece = prop_oneof![
        4 => "[*_`\\[\\]$#>|!()~^:-]{1,40}",
        3 => Just("\n".to_string()),
        1 => Just("\r\n".to_string()),
        1 => Just("\r".to_string()),
        2 => Just("[[".to_string()),
        2 => Just("]]".to_string()),
        2 => Just("![[".to_string()),
        2 => Just("[x](".to_string()),
        1 => Just("](".to_string()),
        2 => Just(")".to_string()),
        1 => Just("[[Note#Head|alias]]".to_string()),
        1 => Just("[t](Some%20Note.md#h)".to_string()),
        1 => Just("![p](<a b.png>)".to_string()),
        1 => Just("[t](a.md \"ti)tle\")".to_string()),
        1 => Just("![p](<a b.png> 'ti\\'tle')".to_string()),
        1 => Just("[t](a.md (title))".to_string()),
        1 => Just("[f](file:///tmp/a.png)".to_string()),
        2 => Just("`".to_string()),
        1 => Just("```\n".to_string()),
        1 => Just("```math\n".to_string()),
        1 => Just("~~~\n".to_string()),
        2 => Just("$".to_string()),
        2 => Just("$$".to_string()),
        1 => Just("$$\n".to_string()),
        1 => Just("\\".to_string()),
        2 => Just("#tag".to_string()),
        1 => Just(" #a/b-c_d ".to_string()),
        1 => Just("# ".to_string()),
        1 => Just("### ".to_string()),
        1 => Just("####### ".to_string()),
        2 => Just("- [ ] ".to_string()),
        1 => Just("1. [x] ".to_string()),
        1 => Just("  * [X] due:2026-01-31 ".to_string()),
        1 => Just("📅 2026-01-31".to_string()),
        2 => Just("[^1]".to_string()),
        1 => Just("[^a_b]: ".to_string()),
        1 => Just("    ".to_string()),
        1 => Just("\t".to_string()),
        2 => Just("---\n".to_string()),
        1 => Just("tags: [a, \"b\"]\n".to_string()),
        1 => Just("numbered-headings: true\n".to_string()),
        1 => Just("key: |\n  v\n- item\n".to_string()),
        1 => Just("<div align=\"center\">".to_string()),
        1 => Just("<div align='RIGHT' >".to_string()),
        2 => Just("</div>".to_string()),
        1 => Just("<div style=\"page-break-after: always;\"></div>".to_string()),
        2 => Just("[@a]".to_string()),
        1 => Just("[see @a, p. 1; -@b]".to_string()),
        2 => Just(" @key".to_string()),
        1 => Just("@ada@host.org".to_string()),
        1 => Just("@fig:x".to_string()),
        2 => Just("{#fig:x}".to_string()),
        1 => Just("{#eq:y}".to_string()),
        1 => Just("{#tbl:z}".to_string()),
        1 => Just("{#sec:s}".to_string()),
        1 => Just("$$ x $$ {#eq:y}".to_string()),
        1 => Just("```math {#eq:y}\n".to_string()),
        1 => Just("type: email\n".to_string()),
        1 => Just("![[p.png]]".to_string()),
        1 => Just("|a|b|\n: cap ".to_string()),
        1 => Just("é".to_string()),
        1 => Just("日本".to_string()),
        1 => Just("😀".to_string()),
        1 => Just("ẞ".to_string()),
        1 => Just(" ".to_string()),
        4 => "\\PC{0,6}",
    ];
    prop::collection::vec(piece, 0..24).prop_map(|parts| parts.concat())
}

/// A target name to write into a link: plain names and the characters a link must escape.
fn new_target() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9 _./é日%()#-]{1,12}"
}

/// A range inside `text`, on char boundaries and not reversed.
fn inside(text: &str, r: &Range<usize>) -> Result<(), String> {
    if r.start <= r.end && r.end <= text.len() && text.is_char_boundary(r.start) && text.is_char_boundary(r.end) {
        Ok(())
    } else {
        Err(format!("{r:?} is not a range on char boundaries of the text (len {})", text.len()))
    }
}

/// Ranges in order and without overlap.
fn in_order(ranges: &[Range<usize>]) -> Result<(), String> {
    match ranges.windows(2).find(|w| w[0].end > w[1].start) {
        Some(w) => Err(format!("{:?} overlaps or follows {:?}", w[0], w[1])),
        None => Ok(()),
    }
}

proptest! {
    #![proptest_config(config())]

    // spec: LINK-018
    #[test]
    fn link_018_found_links_lie_inside_the_text_in_order_and_do_not_overlap(text in md_text()) {
        let links = find_links(&text);
        for l in &links {
            prop_assert_eq!(inside(&text, &l.range), Ok(()));
            prop_assert_eq!(inside(&text, &l.target_range), Ok(()));
            prop_assert!(l.range.start <= l.target_range.start && l.target_range.end <= l.range.end, "{:?}", l);
            // what is read back from the target's range does not panic
            let _ = (written_target(&text, l), l.display(), l.embed_width(), l.is_outside());
        }
        let ranges: Vec<_> = links.iter().map(|l| l.range.clone()).collect();
        prop_assert_eq!(in_order(&ranges), Ok(()));
    }

    // spec: LINK-018
    #[test]
    fn link_018_replacing_a_target_keeps_all_text_outside_it_and_the_link_reads_back(text in md_text(), target in new_target(), pick in any::<prop::sample::Index>()) {
        let links = find_links(&text);
        prop_assume!(!links.is_empty());
        let i = pick.index(links.len());
        let link = &links[i];
        // a wiki link's target is a name that works inside `[[…]]` (`link_safe_name`); a markdown link's escapes what it must
        let target = if link.markdown_style { target } else { link_safe_name(&target) };
        prop_assume!(!target.is_empty());
        let repl = replace_target(&text, link, &target);
        let new = format!("{}{}{}", &text[..link.target_range.start], repl, &text[link.target_range.end..]);
        // all text outside the target stays, byte for byte
        prop_assert!(new.starts_with(&text[..link.target_range.start]));
        prop_assert!(new.ends_with(&text[link.target_range.end..]));
        // the link still reads as one link, now pointing to the new target, and the others are as they were
        let after = find_links(&new);
        prop_assert_eq!(after.len(), links.len(), "{:?} -> {:?}", text, new);
        prop_assert_eq!(after[i].range.start, link.range.start, "{:?} -> {:?}", text, new);
        prop_assert_eq!(written_target(&new, &after[i]), target.clone(), "{:?} -> {:?}", text, new);
        prop_assert_eq!(&after[i].heading, &link.heading);
        prop_assert_eq!(&after[i].alias, &link.alias);
        prop_assert_eq!((after[i].embed, after[i].markdown_style), (link.embed, link.markdown_style));
        for (j, (a, b)) in after.iter().zip(&links).enumerate().filter(|(j, _)| *j != i) {
            prop_assert_eq!((&a.target, &a.heading, &a.alias, a.embed, a.markdown_style), (&b.target, &b.heading, &b.alias, b.embed, b.markdown_style), "link {}", j);
        }
    }

    // spec: LINK-018
    #[test]
    fn link_018_relinking_keeps_the_links_and_changes_only_those_that_match(text in md_text(), target in "[a-zA-Z0-9 é]{0,6}", stem in "[a-zA-Z0-9_é]{1,8}") {
        let links = find_links(&text);
        let matching = links.iter().any(|l| l.target.eq_ignore_ascii_case(strip_md_ext(target.trim())));
        match relink_to(&text, &target, &stem) {
            None => prop_assert!(!matching),
            Some(new) => {
                prop_assert!(matching);
                prop_assert_eq!(find_links(&new).len(), links.len(), "{:?} -> {:?}", text, new);
            }
        }
    }

    // spec: TASK-001
    #[test]
    fn task_001_found_tasks_point_at_their_checkbox_and_toggling_twice_restores_the_text(text in md_text(), at in 0usize..64) {
        let tasks = find_tasks(&text);
        let line_count = text.split('\n').count();
        for t in &tasks {
            prop_assert!(t.line < line_count);
            prop_assert!(text.is_char_boundary(t.check_offset));
            let c = text.as_bytes().get(t.check_offset).copied();
            prop_assert_eq!(c.map(|c| c == b'x' || c == b'X'), Some(t.done), "{:?}", t);
            prop_assert!(matches!(c, Some(b' ' | b'x' | b'X')));
        }
        prop_assert!(tasks.windows(2).all(|w| w[0].line < w[1].line && w[0].check_offset < w[1].check_offset));
        // toggling anywhere does not panic, and twice is the text again
        let mut t = text.clone();
        let offsets = tasks.iter().map(|t| t.check_offset).chain([at, text.len()]);
        for off in offsets {
            let changed = toggle_task_at(&mut t, off);
            if changed {
                prop_assert!(toggle_task_at(&mut t, off));
            }
            // (a `[X]` comes back as `[x]`)
            prop_assert!(t.eq_ignore_ascii_case(&text), "{:?} -> {:?}", text, t);
            t = text.clone();
        }
        for t in &tasks {
            let mut once = text.clone();
            prop_assert!(toggle_task_at(&mut once, t.check_offset));
            prop_assert_eq!(find_tasks(&once).iter().find(|n| n.line == t.line).map(|n| n.done), Some(!t.done));
        }
        for l in text.split('\n') {
            let _ = parse_task_line(l);
        }
    }

    // spec: LINK-018
    #[test]
    fn link_018_found_tags_lie_inside_the_text_in_order_and_name_what_they_cover(text in md_text()) {
        let tags = find_tags(&text);
        for (r, tag) in &tags {
            prop_assert_eq!(inside(&text, r), Ok(()));
            prop_assert_eq!(&text[r.clone()], format!("#{tag}"));
        }
        let ranges: Vec<_> = tags.iter().map(|t| t.0.clone()).collect();
        prop_assert_eq!(in_order(&ranges), Ok(()));
        let _ = frontmatter_tags(&text);
    }

    // spec: EDIT-050
    #[test]
    fn edit_050_footnote_ranges_lie_inside_their_line_and_definitions_do_not_overlap(text in md_text()) {
        let notes = find_footnotes(&text);
        let lines = lines_with_offsets(&text);
        for r in &notes.refs {
            prop_assert!(r.line < lines.len());
            let line = lines[r.line].1;
            prop_assert_eq!(inside(line, &r.range), Ok(()));
            prop_assert_eq!(&line[r.range.clone()], format!("[^{}]", r.label));
        }
        for d in &notes.defs {
            prop_assert!(d.lines.start < d.lines.end && d.lines.end <= lines.len(), "{:?}", d);
            let first = lines[d.lines.start].1;
            prop_assert_eq!(inside(first, &d.marker), Ok(()));
            prop_assert_eq!(&first[d.marker.clone()], format!("[^{}]:", d.label));
            prop_assert!(notes.defined().contains(&d.label));
            prop_assert!(notes.def_at(d.lines.start).is_some());
            prop_assert!((d.lines.start + 1..d.lines.end).all(|l| notes.is_continuation(l)));
        }
        prop_assert!(notes.defs.windows(2).all(|w| w[0].lines.end <= w[1].lines.start));
        prop_assert!(notes.refs.windows(2).all(|w| w[0].line < w[1].line || (w[0].line == w[1].line && w[0].range.end <= w[1].range.start)));
        for (_, line) in &lines {
            for (r, _) in footnote_refs_in_line(line) {
                prop_assert_eq!(inside(line, &r), Ok(()));
            }
            let _ = (footnote_def_marker(line), is_footnote_continuation(line));
        }
    }

    // spec: EDIT-046
    #[test]
    fn edit_046_math_spans_lie_inside_the_text_in_order_and_do_not_overlap(text in md_text()) {
        let spans = find_math(&text);
        let line_count = text.split('\n').count();
        for m in &spans {
            prop_assert_eq!(inside(&text, &m.range), Ok(()));
            prop_assert_eq!(inside(&text, &m.tex), Ok(()));
            prop_assert!(m.range.start <= m.tex.start && m.tex.end <= m.range.end, "{:?}", m);
            prop_assert!(m.lines.start < m.lines.end && m.lines.end <= line_count, "{:?}", m);
            prop_assert!(m.display || !m.block, "{:?}", m);
        }
        let ranges: Vec<_> = spans.iter().map(|m| m.range.clone()).collect();
        prop_assert_eq!(in_order(&ranges), Ok(()));
    }

    // spec: LINK-018
    #[test]
    fn link_018_the_line_scanners_keep_to_the_text(text in md_text()) {
        let lines = lines_with_offsets(&text);
        prop_assert_eq!(lines.len(), text.split('\n').count());
        prop_assert_eq!(fenced_code_lines(&text).len(), lines.len());
        let mut end = 0;
        for (off, line) in &lines {
            prop_assert!(*off >= end || *off == 0);
            prop_assert_eq!(text.get(*off..*off + line.len()), Some(*line));
            end = off + line.len();
            let codes = inline_code_ranges(line);
            for r in &codes {
                prop_assert_eq!(inside(line, r), Ok(()));
                prop_assert!(line[r.clone()].starts_with('`') && line[r.clone()].ends_with('`') && r.len() >= 2);
            }
            prop_assert_eq!(in_order(&codes), Ok(()));
        }
        let front = front_matter_lines(&text);
        prop_assert!(front <= lines.len());
        let _ = front_matter_values(&text);
        let _ = headings_numbered(&text, true);
    }

    // spec: LINK-014
    #[test]
    fn link_014_headings_are_lines_of_the_text_and_numbered_by_nesting(text in md_text()) {
        let heads = find_headings(&text);
        let lines = lines_with_offsets(&text);
        for (line, level, title) in &heads {
            prop_assert!(*line < lines.len());
            prop_assert!((1..=6).contains(level));
            prop_assert_eq!(heading_level(lines[*line].1), Some(*level));
            prop_assert_eq!(title.as_str(), lines[*line].1[*level..].trim());
        }
        prop_assert!(heads.windows(2).all(|w| w[0].0 < w[1].0));
        let numbers = heading_numbers(&text);
        prop_assert_eq!(numbers.len(), heads.len());
        for ((line, level, title, number), (l, lv, t)) in numbers.iter().zip(&heads) {
            prop_assert_eq!((line, level, title), (l, lv, t));
            let n = number.as_deref().expect("a real heading has a number");
            let parts: Vec<usize> = n.split('.').map(|p| p.parse().expect("a number")).collect();
            prop_assert!(parts.iter().all(|p| *p >= 1) && parts.len() <= *level, "{n} at level {level}");
        }
    }

    // spec: EDIT-038
    #[test]
    fn edit_038_lifting_headings_moves_every_heading_up_by_the_same_and_keeps_the_other_text(text in md_text()) {
        let heads = find_headings(&text);
        match lift_headings(&text) {
            None => prop_assert!(heads.iter().map(|h| h.1).min().is_none_or(|m| m == 1)),
            Some(new) => {
                let by = heads.iter().map(|h| h.1).min().unwrap() - 1;
                prop_assert!(by > 0);
                let after = find_headings(&new);
                prop_assert_eq!(after, heads.iter().map(|(l, lv, t)| (*l, lv - by, t.clone())).collect::<Vec<_>>());
                let (old_lines, new_lines) = (lines_with_offsets(&text), lines_with_offsets(&new));
                prop_assert_eq!(old_lines.len(), new_lines.len());
                for (i, ((_, o), (_, n))) in old_lines.iter().zip(&new_lines).enumerate() {
                    if heads.iter().any(|h| h.0 == i) {
                        prop_assert_eq!(&o[by..], *n);
                    } else {
                        prop_assert_eq!(o, n);
                    }
                }
            }
        }
    }

    // spec: PDF-028
    #[test]
    fn pdf_028_wrappers_are_ordered_pairs_of_lines_of_the_text_and_never_nest_or_overlap(text in md_text()) {
        let blocks = align_blocks(&text);
        let lines = lines_with_offsets(&text);
        for b in &blocks {
            prop_assert!(b.open < b.close && b.close < lines.len(), "{:?}", b);
            prop_assert_eq!(align_open_line(lines[b.open].1), Some(b.align));
            prop_assert!(align_close_line(lines[b.close].1));
            prop_assert!(is_align_edge(&blocks, b.open) && is_align_edge(&blocks, b.close));
            prop_assert_eq!(align_at(&blocks, b.open), None);
            prop_assert_eq!(align_at(&blocks, b.open + 1).is_some(), b.holds(b.open + 1));
        }
        prop_assert!(blocks.windows(2).all(|w| w[0].close < w[1].open));
    }

    // spec: PDF-020
    #[test]
    fn pdf_020_page_break_lines_are_increasing_lines_of_the_text_that_are_markers(text in md_text()) {
        let breaks = page_break_lines(&text);
        let lines = lines_with_offsets(&text);
        prop_assert!(breaks.windows(2).all(|w| w[0] < w[1]));
        for i in breaks {
            prop_assert!(i < lines.len() && page_break_line(lines[i].1));
        }
    }

    // spec: LINK-018
    #[test]
    fn link_018_the_small_text_helpers_keep_to_the_text(text in md_text(), other in md_text(), n in 0usize..40, width in any::<u32>()) {
        let stripped = strip_md_ext(&text);
        prop_assert!(text.starts_with(stripped) && (stripped.len() == text.len() || stripped.len() + 3 == text.len()));
        let start = line_start_byte(&text, n);
        prop_assert!(start <= text.len() && text.is_char_boundary(start));
        prop_assert!(start == 0 || start == text.len() || text.as_bytes()[start - 1] == b'\n');
        prop_assert_eq!(byte_to_char(&text, start), text[..start].chars().count());
        prop_assert_eq!(char_to_byte(&text, byte_to_char(&text, start)), start);
        prop_assert_eq!(char_to_byte(&text, n), text.char_indices().nth(n).map_or(text.len(), |c| c.0));
        let _ = byte_to_char(&text, usize::MAX);
        if let Some(at) = find_ci(&text, &other.to_lowercase()) {
            prop_assert!(at <= text.len() && text.is_char_boundary(at));
        }
        let safe = link_safe_name(&text);
        prop_assert!(!safe.contains(['#', '|', '^', '[', ']']));
        prop_assert_eq!(link_safe_name(&safe), safe);
        let _ = (sanitize_note_name(&text), sanitize_note_name_max(&text, n), fuzzy_score(&text, &other));
        let embed = embed_with_width(&text, width);
        if text.ends_with("]]") {
            prop_assert!(embed.ends_with(&format!("|{width}]]")), "{:?} -> {:?}", text, embed);
        } else {
            prop_assert_eq!(embed, text);
        }
    }
}

proptest! {
    #![proptest_config(config_with(64))]

    // spec: LINK-015
    #[test]
    fn link_015_finding_a_heading_by_its_text_does_not_panic_and_finds_a_heading_line(text in md_text(), want in md_text()) {
        let heads = find_headings(&text);
        if let Some(line) = find_heading(&text, &want) {
            prop_assert!(heads.iter().any(|h| h.0 == line));
        }
        if let Some((_, _, title)) = heads.first() {
            prop_assert!(find_heading(&text, title).is_some());
        }
    }
}

// ------------------------------------------------------------------ citations, labels, references (EDIT-122)

mod edit_122 {
    use crate::markdown::citations::{key_len, reserved_prefix, scan, Group, Item, Kind, Label, Target, Token};
    use crate::markdown::find_tags;
    use proptest::prelude::*;

    /// The keys of the bibliography in effect in these tests.
    const HELD: [&str; 8] = ["smith", "jones", "doe2020", "a", "b", "ada", "ada.bsky.social", "key"];

    fn held(key: &str) -> bool {
        HELD.contains(&key)
    }

    /// Every token as the text it covers.
    fn spans(text: &str, bib: bool) -> Vec<&str> {
        let bib: Option<&dyn Fn(&str) -> bool> = bib.then_some(&held);
        scan(text, bib).iter().map(|t| &text[t.range().clone()]).collect()
    }

    fn groups(text: &str, bib: bool) -> Vec<Group> {
        let bib: Option<&dyn Fn(&str) -> bool> = bib.then_some(&held);
        scan(text, bib).into_iter().filter_map(|t| if let Token::Group(g) = t { Some(g) } else { None }).collect()
    }

    fn labels(text: &str) -> Vec<Label> {
        scan(text, None).into_iter().filter_map(|t| if let Token::Label(l) = t { Some(l) } else { None }).collect()
    }

    fn item(g: &Group, i: usize) -> &Item {
        &g.items[i]
    }

    fn cite(key: &str) -> Target {
        Target::Citation(key.into())
    }

    fn reference(kind: Kind, id: &str) -> Target {
        Target::Reference { kind, id: id.into() }
    }

    // spec: EDIT-122
    #[test]
    fn edit_122_the_key_grammar_ends_before_the_full_stop_of_a_sentence() {
        for (text, len) in [("smith", 5), ("smith.", 5), ("smith2020,", 9), ("a.b-c:d+e/f", 11), ("_x", 2), ("1abc", 4), ("a..b", 1), ("a:", 1), ("müller", 7), ("", 0), (".a", 0), ("-a", 0), (":a", 0)] {
            assert_eq!(key_len(text), len, "{text:?}");
        }
        assert_eq!(reserved_prefix("fig:results"), Some(Kind::Figure));
        assert_eq!(reserved_prefix("gls:x"), Some(Kind::Glossary));
        assert_eq!(reserved_prefix("fig:"), None);
        assert_eq!(reserved_prefix("figure:x"), None);
        assert_eq!(reserved_prefix("smith"), None);
    }

    // spec: EDIT-122
    #[test]
    fn edit_122_a_bracketed_citation_with_one_key() {
        let g = groups("As shown [@smith] here.", true);
        assert_eq!(g.len(), 1);
        assert!(g[0].bracketed);
        assert_eq!(item(&g[0], 0).target, cite("smith"));
        assert_eq!(item(&g[0], 0).known, Some(true));
        assert_eq!((item(&g[0], 0).prefix.as_str(), item(&g[0], 0).suffix.as_str(), item(&g[0], 0).suppress_author), ("", "", false));
        assert_eq!(spans("As shown [@smith] here.", true), ["[@smith]"]);
    }

    // spec: EDIT-122
    #[test]
    fn edit_122_a_citation_with_a_locator() {
        let g = groups("[@smith, p. 12]", true);
        assert_eq!((item(&g[0], 0).prefix.as_str(), item(&g[0], 0).suffix.as_str()), ("", "p. 12"));
        assert_eq!(&"[@smith, p. 12]"[item(&g[0], 0).range.clone()], "@smith");
    }

    // spec: EDIT-122
    #[test]
    fn edit_122_a_citation_with_a_prefix_and_a_locator() {
        let g = groups("[see @smith, p. 12]", true);
        assert_eq!((item(&g[0], 0).prefix.as_str(), item(&g[0], 0).suffix.as_str()), ("see", "p. 12"));
    }

    // spec: EDIT-122
    #[test]
    fn edit_122_several_citations_in_one_bracket() {
        let g = groups("[@smith, p. 3; see also @jones; @doe2020]", true);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].items.len(), 3);
        assert_eq!(g[0].items.iter().map(|i| i.target.clone()).collect::<Vec<_>>(), [cite("smith"), cite("jones"), cite("doe2020")]);
        assert_eq!((item(&g[0], 0).suffix.as_str(), item(&g[0], 1).prefix.as_str()), ("p. 3", "see also"));
    }

    // spec: EDIT-122
    #[test]
    fn edit_122_a_bare_citation_where_the_author_is_the_subject() {
        let text = "Then @smith says that it works.";
        let g = groups(text, true);
        assert_eq!(g.len(), 1);
        assert!(!g[0].bracketed);
        assert_eq!(&text[g[0].range.clone()], "@smith");
        assert_eq!(item(&g[0], 0).target, cite("smith"));
        // a full stop after the key is not part of it
        assert_eq!(spans("As @smith.", true), ["@smith"]);
    }

    // spec: EDIT-122
    #[test]
    fn edit_122_a_bare_citation_with_a_locator() {
        let text = "@smith [p. 12] shows it";
        let g = groups(text, true);
        assert_eq!(&text[g[0].range.clone()], "@smith [p. 12]");
        assert_eq!(item(&g[0], 0).suffix, "p. 12");
        assert_eq!(&text[item(&g[0], 0).range.clone()], "@smith");
        // a link after the bracket is no locator
        assert_eq!(spans("@smith [p. 12](x.md)", true), ["@smith"]);
    }

    // spec: EDIT-122
    #[test]
    fn edit_122_a_citation_that_leaves_out_the_author() {
        let text = "[-@smith]";
        let g = groups(text, true);
        assert!(item(&g[0], 0).suppress_author);
        assert_eq!(&text[item(&g[0], 0).range.clone()], "-@smith");
        assert!(!item(&groups("[see -@smith, p. 2]", true)[0], 0).prefix.is_empty());
        // `-@` outside a bracket starts nothing
        assert_eq!(spans("a -@smith b", true), Vec::<&str>::new());
    }

    // spec: EDIT-122
    #[test]
    fn edit_122_references_to_labels_abbreviations_and_terms() {
        for (text, kind, id) in [
            ("see @fig:results", Kind::Figure, "results"),
            ("in @tbl:runs.", Kind::Table, "runs"),
            ("by @eq:energy", Kind::Equation, "energy"),
            ("in @sec:method", Kind::Section, "method"),
            ("the @abbr:ml", Kind::Abbreviation, "ml"),
            ("the @gls:entropy", Kind::Glossary, "entropy"),
        ] {
            let g = groups(text, false);
            assert_eq!(g.len(), 1, "{text}");
            assert_eq!(item(&g[0], 0).target, reference(kind, id), "{text}");
            assert_eq!(item(&g[0], 0).known, None);
        }
        let g = groups("[@fig:a; @fig:b] and [-@fig:c]", false);
        assert_eq!(g.len(), 2);
        assert_eq!(g[0].items.iter().map(|i| i.target.clone()).collect::<Vec<_>>(), [reference(Kind::Figure, "a"), reference(Kind::Figure, "b")]);
        assert!(item(&g[1], 0).suppress_author);
    }

    // spec: EDIT-122
    #[test]
    fn edit_122_a_figure_label_after_an_embed_on_its_own_line() {
        for text in ["![[plot.png|Results of run 2]]{#fig:results}", "![[plot.png|Results]] {#fig:results}", "  ![[plot.png]]{#fig:results}  ", "![a](plot.png){#fig:results}"] {
            let l = labels(text);
            assert_eq!(l.len(), 1, "{text:?}");
            assert_eq!((l[0].kind, l[0].id.as_str()), (Kind::Figure, "results"));
            assert_eq!(&text[l[0].range.clone()], "{#fig:results}");
        }
        for text in ["text ![[plot.png]]{#fig:results}", "![[plot.png]] more {#fig:results}", "![[plot.png]]  {#fig:results}", "[[plot.png]]{#fig:results}", "Fig {#fig:results}"] {
            assert!(labels(text).is_empty(), "{text:?}");
        }
    }

    // spec: EDIT-122
    #[test]
    fn edit_122_a_table_label_in_the_caption_line_below_the_table() {
        let text = "| a | b |\n|---|---|\n| 1 | 2 |\n: The caption {#tbl:runs}\n";
        let l = labels(text);
        assert_eq!((l.len(), l[0].kind, l[0].id.as_str()), (1, Kind::Table, "runs"));
        assert_eq!(&text[l[0].range.clone()], "{#tbl:runs}");
        // not below a table, or not a caption line
        assert!(labels("Some text\n: The caption {#tbl:runs}").is_empty());
        assert!(labels("| a |\n|---|\nThe caption {#tbl:runs}").is_empty());
        assert!(labels("| a |\n|---|\n\n: The caption {#tbl:runs}").is_empty());
    }

    // spec: EDIT-122
    #[test]
    fn edit_122_an_equation_label_after_the_closing_dollars() {
        for text in ["$$ E = mc^2 $$ {#eq:energy}", "$$E=mc^2$${#eq:energy}", "$$\nE = mc^2\n$$ {#eq:energy}", "$$\nE = mc^2\n$${#eq:energy}\nafter", "```math {#eq:energy}\nE = mc^2\n```"] {
            let l = labels(text);
            assert_eq!(l.len(), 1, "{text:?}");
            assert_eq!((l[0].kind, l[0].id.as_str()), (Kind::Equation, "energy"), "{text:?}");
            assert_eq!(&text[l[0].range.clone()], "{#eq:energy}");
        }
        // no label for a formula that is no display, or for two blanks
        assert!(labels("$ x $ {#eq:a}").is_empty());
        assert!(labels("$x$ {#eq:a}").is_empty());
        assert!(labels("$$x$$  {#eq:a}").is_empty());
        assert!(labels("text {#eq:a}").is_empty());
    }

    // spec: EDIT-122
    #[test]
    fn edit_122_a_section_label_at_the_end_of_a_heading() {
        let text = "## Method {#sec:method}\n";
        let l = labels(text);
        assert_eq!((l.len(), l[0].kind, l[0].id.as_str()), (1, Kind::Section, "method"));
        assert_eq!(&text[l[0].range.clone()], "{#sec:method}");
        assert!(labels("Method {#sec:method}").is_empty());
        assert!(labels("## Method {#sec:method} more").is_empty());
        assert!(labels("## Method {#tbl:method}").is_empty(), "a table label does not belong to a heading");
    }

    // spec: EDIT-122
    #[test]
    fn edit_122_rule_1_only_prose_counts_not_front_matter_code_math_or_links() {
        for text in [
            "---\ntitle: [@smith] and @smith\n---\nbody",
            "```\n[@smith] @smith\n```",
            "~~~\n[@smith]\n~~~",
            "`[@smith]` and `@smith`",
            "$a [@smith] b$ and $@smith$",
            "$$ [@smith] @smith $$",
            "$$\n[@smith] @smith\n$$",
            "$$\n[@smith] @smith\n$$ {#eq:x}",
            "```math\n[@smith] @smith\n```",
            "[text](http://example.org/@smith)",
            "[text](@smith.md)",
            "see https://example.org/x/@smith/y",
            "mail mailto:x@smith.org now",
            "<ada@host.org> and <https://x.org/@smith>",
            "<span title=\"@smith\">x</span>",
            "<!-- [@smith] -->",
            "[[@smith]] and ![[@smith.png]]",
            "[[Note#@smith]]",
        ] {
            let found: Vec<&str> = spans(text, true).into_iter().filter(|s| s.contains("smith")).collect();
            assert!(found.is_empty(), "{text:?} gave {found:?}");
        }
        assert_eq!(spans("`code` then [@smith] and $x$", true), ["[@smith]"]);
        // the text after code on the same line is still prose
        assert_eq!(spans("`x` @smith", true), ["@smith"]);
    }

    // spec: EDIT-122
    #[test]
    fn edit_122_rule_1_the_text_of_a_mail_or_a_post_is_somebody_elses() {
        for kind in ["email", "social-post", "Email"] {
            let text = format!("---\ntype: {kind}\n---\n[@smith] @smith @fig:a\n![[p.png]]{{#fig:x}}\n## H {{#sec:h}}\n");
            assert!(scan(&text, None).is_empty(), "{kind}");
            assert!(scan(&text, Some(&held)).is_empty(), "{kind}");
        }
        assert_eq!(spans("---\ntype: note\n---\n[@smith]", true), ["[@smith]"]);
    }

    // spec: EDIT-122
    #[test]
    fn edit_122_rule_2_an_at_starts_only_at_a_word_boundary() {
        for text in ["ada@host.org", "a [x@smith]", "x@smith", "a_@smith", "a.@smith", "a-@smith", "a+@smith", "a/@smith", "1@smith", "é@smith", "[a@smith]", "[see x@smith]", "(see)@smith"] {
            assert!(groups(text, true).is_empty(), "{text:?}");
        }
        for text in ["@smith", "x @smith", "(@smith)", "(see @smith)", "x;@smith", "x,@smith", "\"@smith\"", "'@smith'", "“@smith”", "x\t@smith", "[@smith", "- @smith"] {
            assert_eq!(groups(text, true).len(), 1, "{text:?}");
        }
        // in a bracket too: after a blank, a comma, a bracket or an opening quote
        for text in ["[see @smith]", "[see,@smith]", "[(@smith)]", "[\"@smith\"]", "[x; @smith]", "[x;@smith]"] {
            assert_eq!(groups(text, true).len(), 1, "{text:?}");
        }
    }

    // spec: EDIT-122
    #[test]
    fn edit_122_rule_3_a_mention_is_no_citation() {
        for text in ["@ada@mastodon.social", "hi @ada@mastodon.social!", "[@ada@mastodon.social]", "[see @ada@host]", "@ada@b"] {
            assert!(groups(text, true).is_empty(), "{text:?}");
        }
        // a lone `@` after the key is no mention, an `@` before a non-letter is none either
        assert_eq!(groups("@ada @ x", true).len(), 1);
        assert_eq!(groups("@ada@.", true).len(), 1);
        assert_eq!(groups("[@ada; @b]", true)[0].items.len(), 2);
        // the second item of a bracket may be the one that is a mention
        assert_eq!(groups("[@a; @b@host.org]", true)[0].items.len(), 1);
    }

    // spec: EDIT-122
    #[test]
    fn edit_122_rule_4_a_bare_key_counts_only_when_the_bibliography_holds_it() {
        // no bibliography in effect: plain text
        for text in ["@smith", "@todo", "@here", "@ada.bsky.social"] {
            assert!(groups(text, false).is_empty(), "{text:?}");
        }
        // a bibliography that lacks the key: plain text, no mark
        for text in ["@todo", "@here", "@bob.bsky.social", "@Smith", "@SMITH", "@smit"] {
            assert!(groups(text, true).is_empty(), "{text:?}");
        }
        // a bibliography that holds it, case-sensitive; the key with dots is one key
        assert_eq!(spans("@smith and @ada.bsky.social.", true), ["@smith", "@ada.bsky.social"]);
    }

    // spec: EDIT-122
    #[test]
    fn edit_122_rule_5_a_bracket_is_a_group_unless_it_is_a_link_a_reference_link_or_a_definition() {
        for text in ["[@smith](x.md)", "[@smith](https://x.org)", "[@smith][ref]", "[@smith]: https://x.org", "[see @smith](x)", "![@smith](p.png)"] {
            let found = spans(text, true);
            assert!(!found.iter().any(|s| s.starts_with("[@smith") || s.starts_with("[see")), "{text:?} gave {found:?}");
        }
        // a bracket with `@key` after prefix text is a group
        assert_eq!(spans("[and also @smith in the text]", true), ["[and also @smith in the text]"]);
        // a bracket without a key is no group, and an item without a key is left out
        assert!(groups("[nothing here]", true).is_empty());
        assert!(groups("[a@b.org]", true).is_empty());
        assert_eq!(groups("[for example; @smith]", true)[0].items.len(), 1);
        // a footnote or a task box is no group
        assert!(groups("[^1] [ ] [x]", true).is_empty());
        // nested brackets in the prefix or the locator
        assert_eq!(spans("[see [1] @smith]", true), ["[see [1] @smith]"]);
        // a bracket that is not closed
        assert!(groups("[@smith", true).len() == 1 && !groups("[@smith", true)[0].bracketed);
    }

    // spec: EDIT-122
    #[test]
    fn edit_122_rule_5_unknown_keys_are_marked_and_without_a_bibliography_the_group_is_plain() {
        let g = groups("[@smith; @missing]", true);
        assert_eq!((item(&g[0], 0).known, item(&g[0], 1).known), (Some(true), Some(false)));
        let g = groups("[@smith; @missing]", false);
        assert_eq!((item(&g[0], 0).known, item(&g[0], 1).known), (None, None), "no bibliography: the group is plain text, the TeX export warns");
        // a key with other case than the one the bibliography holds
        assert_eq!(item(&groups("[@Smith]", true)[0], 0).known, Some(false));
    }

    // spec: EDIT-122
    #[test]
    fn edit_122_rule_6_the_reserved_prefixes_always_make_a_reference() {
        for bib in [false, true] {
            for prefix in ["fig", "tbl", "eq", "sec", "abbr", "gls"] {
                let text = format!("@{prefix}:x and [@{prefix}:y]");
                let g = groups(&text, bib);
                assert_eq!(g.len(), 2, "{text} {bib}");
                assert!(matches!(item(&g[0], 0).target, Target::Reference { .. }) && matches!(item(&g[1], 0).target, Target::Reference { .. }));
            }
        }
        // `fig:` alone, or another word before the colon, is a key
        assert!(groups("@fig", true).is_empty());
        assert_eq!(item(&groups("[@figure:x]", true)[0], 0).target, cite("figure:x"));
        // the bibliography holding such a key does not make it a citation
        let has_all = |_: &str| true;
        let g = scan("@fig:x", Some(&has_all));
        assert!(matches!(&g[0], Token::Group(g) if matches!(g.items[0].target, Target::Reference { .. })));
    }

    // spec: EDIT-122
    #[test]
    fn edit_122_rule_7_a_label_is_read_only_at_its_places_and_is_no_tag() {
        for text in ["text {#fig:x} text", "a {#sec:x}", "{#fig:x}", "## H {#fig:x}", "`![[a.png]]{#fig:x}`", "{#other:x}", "{#fig:}", "{#fig x}", "## H {#sec:a b}", "## H {# sec:x}", "## H {#sec:x"] {
            assert!(labels(text).is_empty(), "{text:?}");
        }
        assert_eq!(labels("## H {#sec:a_b-c.d:e}")[0].id, "a_b-c.d:e");
        // `{#…}` is no tag, as the tag rule needs a blank, a bracket or the line start before the `#`
        for text in ["{#fig:x}", "## H {#sec:x}", "a {#abc} b", "{#abc}"] {
            assert!(find_tags(text).is_empty(), "{text:?}");
        }
        assert_eq!(find_tags("a #real {#fig:x}").len(), 1);
    }

    // spec: EDIT-122
    #[test]
    fn edit_122_rule_8_a_backslash_makes_the_at_a_plain_character() {
        assert!(groups("\\@smith", true).is_empty());
        assert!(groups("[\\@smith]", true).is_empty());
        assert!(groups("\\@fig:x", false).is_empty());
    }

    // spec: EDIT-122
    #[test]
    fn edit_122_the_tokens_come_in_the_order_of_the_text_and_are_kept_to_the_text() {
        let text = "## Intro {#sec:intro}\nFirst [@smith, p. 1] then @jones and @fig:a.\n\n![[a.png]]{#fig:a}\nÄ ünï [@doe2020]\n";
        let tokens = scan(text, Some(&held));
        let found: Vec<&str> = tokens.iter().map(|t| &text[t.range().clone()]).collect();
        assert_eq!(found, ["{#sec:intro}", "[@smith, p. 1]", "@jones", "@fig:a", "{#fig:a}", "[@doe2020]"]);
        assert!(tokens.windows(2).all(|w| w[0].range().end <= w[1].range().start));
        // windows line endings
        let crlf = text.replace('\n', "\r\n");
        assert_eq!(scan(&crlf, Some(&held)).len(), 6);
    }

    // spec: EDIT-122
    #[test]
    fn edit_122_a_labelled_display_formula_hides_its_body_from_the_scanner() {
        use crate::markdown::find_math;
        let text = "$$\n[@smith] x@y\n$$ {#eq:x}\n@smith\n";
        let math = find_math(text);
        assert_eq!(math.len(), 1);
        assert_eq!(&text[math[0].range.clone()], "$$\n[@smith] x@y\n$$");
        assert_eq!(spans(text, true), ["{#eq:x}", "@smith"]);
        // a math fence with a label in its info string is math, and another word is not
        let fence = "```math {#eq:y}\na\n```\n```math more\n[@smith]\n```";
        assert_eq!(find_math(fence).len(), 1);
    }

    proptest! {
        #![proptest_config(crate::property_test_support::config())]

        // spec: EDIT-122, LINK-018
        #[test]
        fn edit_122_the_scanner_never_panics_and_its_ranges_lie_inside_the_text_in_order(text in super::md_text()) {
            for bib in [None, Some(&held as &dyn Fn(&str) -> bool)] {
                let tokens = scan(&text, bib);
                for t in &tokens {
                    prop_assert_eq!(super::inside(&text, t.range()), Ok(()));
                    if let Token::Group(g) = t {
                        prop_assert!(!g.items.is_empty());
                        for i in &g.items {
                            prop_assert_eq!(super::inside(&text, &i.range), Ok(()));
                            prop_assert!(g.range.start <= i.range.start && i.range.end <= g.range.end, "{:?}", g);
                            prop_assert!(text[i.range.clone()].contains('@'));
                        }
                    }
                }
                let ranges: Vec<_> = tokens.iter().map(|t| t.range().clone()).collect();
                prop_assert_eq!(super::in_order(&ranges), Ok(()));
            }
        }
    }
}
