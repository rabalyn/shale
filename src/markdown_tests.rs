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
