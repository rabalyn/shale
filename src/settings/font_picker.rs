//! The font picker (CFG-032): a button that opens a list of the installed font families with a
//! field on top that filters it. Typing narrows the list (the matching of the file picker,
//! `markdown::fuzzy_score`), ↓ / ↑ move, Enter picks, Esc closes, a click picks. The first
//! entry is "Default", Shale's own font. Every area of the Fonts tab uses it. The list opens on
//! the chosen font, the mouse highlights the entry it moves over, and the highlighted entry is
//! handed back for the sample text of the row (CFG-040). Each entry is drawn in its own font
//! (CFG-077): the rows on screen ask `FontState` for their faces, which come in batches, and a
//! row whose face is not there yet shows its name in Shale's font.

use crate::font_list::SYMBOL_SAMPLE;
use crate::fonts::{FontState, ListLook};
use crate::keys::Keymap;
use crate::markdown;
use crate::picker::ListKeys;
use eframe::egui::{self, text::LayoutJob, Popup, PopupCloseBehavior, TextFormat, Ui, WidgetText};

/// The name of the first entry, which picks Shale's own font.
pub const DEFAULT_ENTRY: &str = "Default";

/// How tall the list of families is at most.
const LIST_HEIGHT: f32 = 260.0;

/// What the filter field says and where in the list the pick is.
#[derive(Clone, Default)]
struct State {
    query: String,
    sel: usize,
    /// the scroll position of the list
    offset: f32,
    /// the popup was just opened: the field takes the keyboard
    fresh: bool,
}

/// The lines of the list for what is typed: `None` is the "Default" entry, which is always first.
/// The families that match come after it, the best match first.
pub fn entries<'a>(families: &'a [String], query: &str) -> Vec<Option<&'a str>> {
    let q = query.trim();
    let mut out = Vec::with_capacity(families.len() + 1);
    if q.is_empty() || DEFAULT_ENTRY.to_lowercase().contains(&q.to_lowercase()) {
        out.push(None);
    }
    if q.is_empty() {
        out.extend(families.iter().map(|f| Some(f.as_str())));
    } else {
        let mut hits: Vec<(i32, &str)> = families.iter().filter_map(|f| markdown::fuzzy_score(q, f).map(|s| (s, f.as_str()))).collect();
        hits.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.to_lowercase().cmp(&b.1.to_lowercase())));
        out.extend(hits.into_iter().map(|(_, f)| Some(f)));
    }
    out
}

/// The text of an entry in the way the font allows (CFG-077): in its own font; as the name in
/// Shale's font followed by a sample in the font when its letters are pictures; in Shale's font
/// while the font is not there.
pub(crate) fn entry_text(ui: &Ui, label: &str, look: &ListLook) -> WidgetText {
    match look {
        ListLook::Fallback => label.into(),
        ListLook::Own { family, symbol: false } => egui::RichText::new(label).family(family.clone()).into(),
        ListLook::Own { family, symbol: true } => {
            let own = egui::TextStyle::Button.resolve(ui.style());
            let mut job = LayoutJob::default();
            job.append(&format!("{label}  "), 0.0, TextFormat { font_id: own.clone(), ..Default::default() });
            job.append(SYMBOL_SAMPLE, 0.0, TextFormat { font_id: egui::FontId::new(own.size, family.clone()), ..Default::default() });
            job.into()
        }
    }
}

/// What happened in the picker in this frame.
pub struct Outcome {
    /// where the button of the picker is
    pub rect: egui::Rect,
    /// a font was picked
    pub picked: bool,
    /// while the list is open: the entry that is highlighted (`None` in it: "Default")
    pub highlighted: Option<Option<String>>,
}

/// The picker for `current` (`None`: Shale's own font). `families` is the list of installed
/// fonts; `searching` says it is not complete yet.
pub fn show(ui: &mut Ui, salt: &str, current: &mut Option<String>, families: &[String], searching: bool, keymap: &Keymap, fonts: &mut FontState) -> Outcome {
    let id = ui.make_persistent_id(("font-picker", salt));
    let shown = current.as_deref().unwrap_or(DEFAULT_ENTRY);
    let button = ui.add(egui::Button::new(format!("{shown}  ⏷")).min_size(egui::vec2(220.0, 0.0))).on_hover_text("Choose the font; type to search the installed fonts");
    if button.clicked() {
        // the list opens on the chosen font
        let sel = entries(families, "").iter().position(|e| e.map(str::to_string) == *current).unwrap_or(0);
        let row = ui.text_style_height(&egui::TextStyle::Button) + ui.spacing().item_spacing.y + ui.spacing().button_padding.y * 2.0;
        let offset = (sel as f32 * row - LIST_HEIGHT / 2.0).max(0.0);
        ui.data_mut(|d| d.insert_temp(id, State { fresh: true, sel, offset, ..State::default() }));
    }
    let popup_id = Popup::default_response_id(&button);
    let mut picked = false;
    let mut highlighted = None;
    Popup::from_toggle_button_response(&button).close_behavior(PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
        ui.set_min_width(260.0);
        let mut st: State = ui.data(|d| d.get_temp(id)).unwrap_or_default();
        let opened = st.fresh;
        let keys = ListKeys::take(ui, keymap);
        let list = entries(families, &st.query);
        let before = st.sel;
        keys.select(&mut st.sel, list.len());
        let moved = st.sel != before;
        let field = ui.add(egui::TextEdit::singleline(&mut st.query).id(id.with("query")).hint_text("Search fonts…").desired_width(f32::INFINITY));
        if st.fresh {
            field.request_focus();
            st.fresh = false;
        }
        if field.changed() {
            st.sel = 0;
            st.offset = 0.0;
        }
        let list = entries(families, &st.query);
        st.sel = st.sel.min(list.len().saturating_sub(1));
        if keys.enter {
            if let Some(entry) = list.get(st.sel) {
                *current = entry.map(str::to_string);
                picked = true;
            }
        }
        if searching {
            ui.label(egui::RichText::new("Looking for the installed fonts…").small().color(crate::theme::palette().muted));
        }
        if list.is_empty() {
            ui.label(egui::RichText::new("No font matches").small().color(crate::theme::palette().muted));
        }
        let row_h = ui.text_style_height(&egui::TextStyle::Button) + ui.spacing().item_spacing.y + ui.spacing().button_padding.y * 2.0;
        let mut area = egui::ScrollArea::vertical().id_salt(id.with("list")).max_height(LIST_HEIGHT).auto_shrink([false, true]);
        if opened {
            area = area.vertical_scroll_offset(st.offset);
        } else if moved {
            let top = st.sel as f32 * row_h;
            if top < st.offset {
                area = area.vertical_scroll_offset(top);
            } else if top + row_h > st.offset + LIST_HEIGHT {
                area = area.vertical_scroll_offset(top + row_h - LIST_HEIGHT);
            }
        }
        let mut hover = None;
        let mut visible = Vec::new();
        let now = ui.input(|inp| inp.time);
        let out = area.show_rows(ui, row_h, list.len(), |ui, range| {
            for i in range {
                let name = list[i].unwrap_or(DEFAULT_ENTRY);
                let on = list[i].map(str::to_string) == *current;
                let label = if on { format!("{name}  ✓") } else { name.to_string() };
                let look = match list[i] {
                    Some(f) => {
                        visible.push(f.to_string());
                        fonts.list_look(f)
                    }
                    None => ListLook::Fallback,
                };
                let text = entry_text(ui, &label, &look);
                let row = ui.add_sized([ui.available_width(), row_h - ui.spacing().item_spacing.y], egui::Button::selectable(i == st.sel, text));
                if row.clicked() {
                    *current = list[i].map(str::to_string);
                    picked = true;
                }
                // the mouse moving over an entry highlights it
                if row.hovered() && ui.input(|inp| inp.pointer.delta() != egui::Vec2::ZERO) {
                    hover = Some(i);
                }
            }
        });
        fonts.list_want(visible, now);
        if let Some(i) = hover {
            st.sel = i;
        }
        highlighted = list.get(st.sel).map(|e| e.map(str::to_string));
        st.offset = out.state.clamped_offset().y;
        ui.data_mut(|d| d.insert_temp(id, st));
        if keys.esc || picked {
            Popup::close_id(ui.ctx(), popup_id);
        }
    });
    Outcome { rect: button.rect, picked, highlighted: if picked { None } else { highlighted } }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fams() -> Vec<String> {
        ["DejaVu Sans", "DejaVu Serif", "Noto Sans", "Virgil", "Ubuntu"].iter().map(|s| s.to_string()).collect()
    }

    // spec: CFG-032
    #[test]
    fn cfg_032_the_list_starts_with_default_and_is_filtered_by_what_is_typed() {
        let f = fams();
        let all = entries(&f, "");
        assert_eq!((all.len(), all[0], all[1]), (6, None, Some("DejaVu Sans")));
        assert_eq!(entries(&f, "serif"), [Some("DejaVu Serif")], "case does not matter");
        assert_eq!(entries(&f, "NOTO"), [Some("Noto Sans")]);
        assert!(entries(&f, "zzz").is_empty());
        assert_eq!(entries(&f, "defa")[0], None, "typing 'default' keeps the Default entry");
        // the best match first
        assert_eq!(entries(&f, "dvsans")[0], Some("DejaVu Sans"));
    }

    // spec: CFG-077
    #[test]
    fn cfg_077_a_symbol_font_shows_its_name_in_shale_s_font_and_a_sample_in_the_font() {
        let ctx = egui::Context::default();
        let fam = egui::FontFamily::Name("system:Sym".into());
        let own = |symbol| crate::fonts::ListLook::Own { family: fam.clone(), symbol };
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 300.0))), ..Default::default() };
        let mut seen = Vec::new();
        let mut out = ctx.run_ui(raw, |ui| {
            for look in [crate::fonts::ListLook::Fallback, own(false), own(true)] {
                seen.push(entry_text(ui, "Sym", &look));
            }
        });
        out.textures_delta.clear();
        assert!(matches!(&seen[0], egui::WidgetText::Text(t) if t == "Sym"), "the fallback is plain text in Shale's font");
        assert!(matches!(&seen[1], egui::WidgetText::RichText(_)), "a font that shows its name draws it in the font");
        let egui::WidgetText::LayoutJob(job) = &seen[2] else { panic!("a symbol font is a job of two parts") };
        assert_eq!(job.sections.len(), 2);
        assert_eq!(job.text, format!("Sym  {}", crate::font_list::SYMBOL_SAMPLE));
        assert_ne!(job.sections[0].format.font_id.family, fam, "the name is in Shale's font");
        assert_eq!(job.sections[1].format.font_id.family, fam, "the sample is in the symbol font");
    }
}
