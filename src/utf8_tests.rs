//! Every text function gets text with umlauts, ß, € and emoji at every position (and every
//! cursor position): none of them may cut a character in half (which would crash the app).

use crate::{cal_ics, editor, import, lists, mail, markdown, media, pdfexport, reader};
use eframe::egui::{self, Pos2, RawInput, Rect, Vec2};
use std::path::{Path, PathBuf};

/// Typical lines, each with non-ASCII characters next to every kind of marker.
fn samples() -> Vec<String> {
    let base = [
        "[[Müll]] und [[Müll|Mülltonne]] und [[Müll#Überschrift]] ![[Bild ä.png]] [[ü",
        "- [ ] Müll rausbringen 📅 2026-12-0ä due:Müllabfuhr @due(ü)",
        "1. [x] Grüße ✓ #tägliches #ü/ä #123 `cödé` **fëtt** *kürsiv* ~~weg~~ ==hell==",
        "# Überschrift\n## Ärger\n### €uro",
        "[Straße](https://example.org/ä) [ö](Notiz%C3%BC.md#Kapitel ü) <ä@ö.de>",
        "---\ntags: [ä, ö]\ntitle: \"Grüße\"\n---\n    ```\nä\n```",
        "> Zitat mit ß\n\t- Einrückung ✓\n  * Punkt €\n- [ ] [!~/Bilder/ä[[p Mül",
        "📅📅📅📅📅📅📅📅📅📅 ẞ̈ 𝔘𝔫𝔦𝔠𝔬𝔡𝔢 a\u{0308}",
    ];
    let mut out: Vec<String> = base.iter().map(|s| s.to_string()).collect();
    // also each line cut off after every character (half-typed markup)
    for s in base {
        for (i, _) in s.char_indices() {
            out.push(s[..i].to_string());
        }
    }
    out
}

fn boundaries(s: &str) -> Vec<usize> {
    s.char_indices().map(|(i, _)| i).chain(std::iter::once(s.len())).collect()
}

// spec: VAULT-010, DATA-006
#[test]
fn text_functions_never_cut_a_character() {
    for s in samples() {
        let _ = markdown::find_links(&s);
        let _ = markdown::find_tasks(&s);
        let _ = markdown::find_tags(&s);
        let _ = markdown::frontmatter_tags(&s);
        let _ = markdown::find_headings(&s);
        let _ = markdown::strip_md_ext(&s);
        let _ = markdown::link_safe_name(&s);
        let _ = markdown::sanitize_note_name(&s);
        let _ = markdown::fuzzy_score("mü", &s);
        for l in markdown::find_links(&s) {
            let _ = markdown::written_target(&s, &l);
            let _ = markdown::replace_target(&s, &l, "Müllabfuhr");
        }
        for l in s.lines() {
            let _ = markdown::parse_task_line(l);
            let _ = markdown::heading_level(l);
            let _ = lists::list_prefix(l);
        }
        let _ = lists::find_web_links(&s);
        let links: Vec<_> = markdown::find_links(&s).into_iter().map(|l| (l, true)).collect();
        let _ = editor::highlight(&s, &links, 15.0);
        let _ = import::parse_tags(&s);
        #[cfg(target_os = "linux")]
        {
            let _ = import::text_of(s.as_bytes());
            let _ = import::items_from_drop("text/plain", s.as_bytes());
        }
        let _ = mail::decode_words(&s);
        let _ = mail::html_to_text(&s);
        let _ = mail::parse(s.as_bytes());
        let _ = mail::parse_date(&s);
        let _ = mail::display_name(&s);
        let _ = cal_ics::parse_ics(&s);
        let _ = crate::cal_dialog::parse_time_text(&s);
        let _ = media::split_typed(&s, Path::new("/v"), Path::new("/h"));
        for (k, b) in boundaries(&s).into_iter().enumerate() {
            let _ = lists::continue_list(&s, b);
            let _ = lists::indent(&s, 0..b, false);
            let _ = lists::indent(&s, b..s.len(), true);
            let _ = lists::move_lines(&s, b..b, true);
            let _ = lists::move_lines(&s, b..s.len(), false);
            let _ = media::media_query(&s, b);
            let _ = crate::link_assist::link_query(&s, k);
            let _ = markdown::byte_to_char(&s, b);
            let _ = markdown::char_to_byte(&s, k);
        }
        for b in boundaries(&s) {
            let mut t = s.clone();
            let _ = markdown::toggle_task_at(&mut t, b);
        }
    }
}

// spec: DATA-006
#[test]
fn reading_view_and_pdf_export_take_any_text() {
    let d = std::env::temp_dir().join(format!("shale-utf8-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("Müll.md"), "# Müll\n").unwrap();
    let v = crate::vault::Vault::open(&d).unwrap();
    let mut scenes = reader::SceneCache::default();
    let ctx = egui::Context::default();
    let opts = pdfexport::ExportOptions::for_note();
    for s in samples().into_iter().step_by(3) {
        let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(500.0, 400.0))), ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| {
            reader::show(ui, &s, &v, &PathBuf::from("Müll.md"), &mut scenes, 15.0, true);
        });
        out.textures_delta.clear();
        let _ = pdfexport::export_note(&v, Path::new("Müll.md"), &s, &opts);
    }
    let _ = std::fs::remove_dir_all(&d);
}
