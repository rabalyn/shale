//! The window of the form (QUERY-021…QUERY-025): the rows of the filter, the sort rows, and the
//! Advanced switch that swaps them for the text.

use super::complete::{Complete, Slot, Spec};
use super::sort::{self, Key};
use super::{DueCmp, Fit, Mode, Row, State, What};
use crate::search_all::query::Cache;
use crate::search_sources::{self, Prefixes};
use crate::theme;
use crate::vault::Vault;
use eframe::egui::{self, Align, Id, Layout, RichText, Sense, Ui};

/// What the user changed in this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Changed {
    /// the text of the query
    pub query: bool,
    /// the text of the sort
    pub sort: bool,
    /// the Advanced switch, which is a setting (QUERY-026): save it
    pub advanced: bool,
}

/// What the host of the form says about itself.
pub struct Opts<'a> {
    /// the vault, for the lists of the tags and properties it knows (QUERY-029)
    pub vault: &'a Vault,
    /// the letters of the prefixes (NAV-050); none in a lane
    pub prefixes: Prefixes,
    /// The host has a text box of its own for the query (the picker, the sidebar): Advanced adds
    /// none, and the host shows the error of the query.
    pub host_box: bool,
    /// the live count of the matches, for Advanced (QUERY-024); the host has it from its search
    pub count: Option<&'a str>,
}

/// A one-line text field of the form; a quote is never typed (the language has no way to say it).
fn edit(ui: &mut Ui, value: &mut String, hint: &str, width: f32) -> bool {
    let r = ui.add(egui::TextEdit::singleline(value).hint_text(hint).desired_width(width));
    finish(r, value)
}

/// The last field of a row: it takes the rest of the row, at least `min` wide.
fn edit_rest(ui: &mut Ui, value: &mut String, hint: &str, min: f32) -> bool {
    let width = (ui.available_width() - 8.0).max(min);
    edit(ui, value, hint, width)
}

fn finish(r: egui::Response, value: &mut String) -> bool {
    if r.changed() && value.contains('"') {
        value.retain(|c| c != '"');
    }
    r.changed()
}

/// The value fields of one row; true when one changed.
fn fields(ui: &mut Ui, what: &mut What, i: usize, comp: &mut Complete, vault: &Vault) -> bool {
    fn spec<'a>(row: usize, slot: Slot, key: &'a str, hint: &'a str, width: f32) -> Spec<'a> {
        Spec { row, slot, key, hint, width }
    }
    match what {
        What::Tag(v) => {
            let width = (ui.available_width() - 8.0).max(100.0);
            comp.field(ui, vault, &spec(i, Slot::Tag, "", "tag", width), v)
        }
        What::Prop { key, value } => {
            let changed = comp.field(ui, vault, &spec(i, Slot::Key, "", "name", 90.0), key);
            let width = (ui.available_width() - 8.0).max(90.0);
            changed | comp.field(ui, vault, &spec(i, Slot::Value, key, "value", width), value)
        }
        What::Due { cmp, day } => {
            let before = *cmp;
            egui::ComboBox::from_id_salt(("due-cmp", i)).selected_text(cmp.label()).width(80.0).show_ui(ui, |ui| {
                for c in DueCmp::ALL {
                    ui.selectable_value(cmp, c, c.label());
                }
            });
            let mut changed = *cmp != before;
            if *cmp != DueCmp::Overdue {
                changed |= edit_rest(ui, day, "2026-12-01, today, +7d", 130.0);
            }
            changed
        }
        What::Blocked | What::Blocking => false,
        What::Folder(v) => edit_rest(ui, v, "Folder/", 110.0),
        What::Links(v) => edit_rest(ui, v, "Note", 110.0),
        What::Text(v) => edit_rest(ui, v, "text, * and ? are wildcards", 150.0),
    }
}

/// Take an entry of a list into the field it is for.
fn take_entry(what: &mut What, slot: Slot, entry: String) {
    match (what, slot) {
        (What::Tag(v), Slot::Tag) | (What::Prop { key: v, .. }, Slot::Key) | (What::Prop { value: v, .. }, Slot::Value) => *v = entry,
        _ => {}
    }
}

/// A button that opens a list of choices under it. It is a popup, not a menu: a menu inside the
/// popover of a lane closes the popover with the choice (#632), and a choice here only closes the list.
fn add_menu(ui: &mut Ui, label: &str, choices: impl FnOnce(&mut Ui)) {
    let button = ui.button(label);
    egui::Popup::from_toggle_button_response(&button).close_behavior(egui::PopupCloseBehavior::CloseOnClick).show(|ui| {
        ui.set_min_width(110.0);
        choices(ui);
    });
}

/// The filter rows (QUERY-021); true when the rows changed.
fn filter_rows(ui: &mut Ui, st: &mut State, vault: &Vault) -> bool {
    let mut edited = false;
    st.complete.begin();
    let mut remove = None;
    let comp = &mut st.complete;
    for (i, row) in st.rows.iter_mut().enumerate() {
        ui.push_id(("filter-row", i), |ui| {
            // (not wrapped: the last field takes the rest of the line)
            ui.horizontal(|ui| {
                if ui.small_button("✕").on_hover_text("Remove this filter").clicked() {
                    remove = Some(i);
                }
                ui.label(RichText::new(row.what.label()).color(theme::palette().muted));
                edited |= ui.checkbox(&mut row.not, "not").changed();
                edited |= fields(ui, &mut row.what, i, comp, vault);
            });
            if let Some((slot, entry)) = comp.list(ui, i) {
                take_entry(&mut row.what, slot, entry);
                edited = true;
            }
            if let Err(why) = row.term() {
                ui.label(RichText::new(why).color(theme::palette().muted).size(11.5));
            }
        });
    }
    if let Some(i) = remove {
        st.rows.remove(i);
        edited = true;
    }
    add_menu(ui, "+ Add filter", |ui| {
        for what in What::blanks() {
            if ui.button(what.label()).clicked() {
                st.rows.push(Row { not: false, what });
                edited = true;
                ui.close();
            }
        }
    });
    edited
}

/// The sort rows (QUERY-023); true when they changed.
fn sort_rows(ui: &mut Ui, st: &mut sort::State) -> bool {
    let mut edited = false;
    let (mut remove, mut moved) = (None, None);
    let n = st.keys.len();
    for i in 0..n {
        let row = ui.push_id(("sort-key", i), |ui| {
            ui.horizontal(|ui| {
                ui.dnd_drag_source(Id::unique(("sort-handle", i)), i, |ui| ui.label("⠿")).response.on_hover_text("Drag to reorder");
                let used: Vec<sort::Field> = st.keys.iter().map(|k| k.field).collect();
                let key = &mut st.keys[i];
                let before = key.field;
                let others: Vec<_> = sort::Field::ALL.into_iter().filter(|f| *f == key.field || !used.contains(f)).collect();
                egui::ComboBox::from_id_salt(("sort-field", i)).selected_text(key.field.word()).width(110.0).show_ui(ui, |ui| {
                    for f in others {
                        ui.selectable_value(&mut key.field, f, f.word());
                    }
                });
                if key.field != before {
                    *key = Key::new(key.field, key.desc);
                    edited = true;
                }
                if key.field.directed() && ui.button(if key.desc { "desc" } else { "asc" }).on_hover_text("Click to turn the direction round").clicked() {
                    key.desc = !key.desc;
                    edited = true;
                }
                if ui.add_enabled(i > 0, egui::Button::new("▲").small()).on_hover_text("Move up").clicked() {
                    moved = Some((i, i - 1));
                }
                if ui.add_enabled(i + 1 < n, egui::Button::new("▼").small()).on_hover_text("Move down").clicked() {
                    moved = Some((i, i + 1));
                }
                if ui.small_button("✕").on_hover_text("Remove this sort").clicked() {
                    remove = Some(i);
                }
            });
        });
        // a row dragged by its handle lands here
        let zone = ui.interact(row.response.rect, Id::unique(("sort-drop", i)), Sense::hover());
        if zone.dnd_hover_payload::<usize>().is_some() {
            ui.painter().hline(row.response.rect.x_range(), row.response.rect.bottom(), egui::Stroke::new(2.0, theme::palette().accent));
        }
        if let Some(from) = zone.dnd_release_payload::<usize>() {
            moved = Some((*from, i));
        }
    }
    if let Some((from, to)) = moved {
        st.move_key(from, to);
        edited = true;
    }
    if let Some(i) = remove {
        st.keys.remove(i);
        edited = true;
    }
    let unused = st.unused();
    if !unused.is_empty() {
        add_menu(ui, "+ Add sort", |ui| {
            for f in unused {
                if ui.button(f.word()).clicked() {
                    st.keys.push(Key::new(f, false));
                    edited = true;
                    ui.close();
                }
            }
        });
    }
    edited
}

/// Draw the form of the query `text`, and of the sort text `sort` when the host has one (a lane).
/// It edits the text it is given, and says what it changed. `cache` checks the text (QUERY-013);
/// `advanced` is the remembered Advanced switch (QUERY-026), which the form turns on by itself
/// while the query does not fit it (QUERY-025).
pub fn show(ui: &mut Ui, st: &mut State, text: &mut String, cache: &mut Cache, advanced: &mut bool, sort: Option<&mut String>, opts: &Opts) -> Changed {
    let mut changed = Changed::default();
    let checked = cache.get(text, opts.prefixes).clone();
    st.sync(text, &checked);
    if let Some(s) = &sort {
        st.sort.sync(s);
    }
    let fits = st.fit == Fit::Rows;
    let as_text = *advanced || !fits;
    let focus = std::mem::take(&mut st.focus);
    let mut first = None;
    let mut mode_clicked = false;
    ui.horizontal(|ui| {
        if !as_text {
            for (mode, label) in [(Mode::All, "All of"), (Mode::Any, "Any of")] {
                let r = ui.selectable_label(st.mode == mode, label).on_hover_text(if mode == Mode::All { "A note must match all the filters" } else { "A note must match one of the filters" });
                if r.clicked() && st.mode != mode {
                    st.mode = mode;
                    mode_clicked = true;
                }
                first.get_or_insert(r);
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let tip = if fits { "Edit the query as text" } else { "A query that does not fit the form is edited as text" };
            let r = ui.add_enabled(fits, egui::Button::new("Advanced").selected(as_text)).on_hover_text(tip).on_disabled_hover_text(tip);
            if r.clicked() {
                *advanced = !*advanced;
                changed.advanced = true;
            }
            if as_text && opts.host_box {
                first.get_or_insert(r);
            }
        });
    });
    if as_text {
        match &st.fit {
            Fit::Complex => {
                theme::paragraph(ui, "This query is too complex for the form");
            }
            Fit::Invalid(_) => {
                theme::hint(ui, "The form shows a valid query. Fix the text first.");
            }
            Fit::Rows => {}
        }
        if opts.host_box {
            theme::hint(ui, "Edit the query in the box above.");
        } else {
            let mut layouter = search_sources::error_layouter(cache, opts.prefixes, None);
            let r = ui.add(egui::TextEdit::singleline(text).hint_text("status:doing OR #todo").desired_width(f32::INFINITY).layouter(&mut layouter));
            changed.query |= r.changed();
            first.get_or_insert(r);
            if let Some(e) = checked.error() {
                theme::problem(ui, e.reason.clone());
            }
        }
        if let Some(count) = opts.count {
            theme::hint(ui, count);
        }
    } else {
        let edited = filter_rows(ui, st, opts.vault) | mode_clicked;
        if !opts.host_box {
            if let Some(e) = checked.error() {
                theme::problem(ui, e.reason.clone());
            }
        }
        if edited {
            let new = st.written();
            if new != *text {
                *text = new;
                changed.query = true;
            }
        }
    }
    if let Some(sort) = sort {
        ui.add_space(4.0);
        ui.label(RichText::new("Sort").color(theme::palette().muted));
        if as_text || st.sort.error.is_some() {
            changed.sort |= ui.add(egui::TextEdit::singleline(sort).hint_text("due asc, priority desc").desired_width(f32::INFINITY)).changed();
            if let Some(e) = &st.sort.error {
                theme::problem(ui, e.clone());
            }
        } else if sort_rows(ui, &mut st.sort) {
            let new = st.sort.written();
            if new != *sort {
                *sort = new;
                changed.sort = true;
            }
        }
    }
    if focus {
        if let Some(r) = first {
            r.request_focus();
        }
    }
    if changed.query || changed.sort {
        ui.ctx().request_repaint();
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::super::complete::list_open;
    use super::*;
    use crate::search_all::query::Cache;
    use crate::search_form::sort::{self, Field};
    use crate::search_form::DueCmp;
    use eframe::egui::{accesskit::Role, Event, Key, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2};

    static TEST_VAULTS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    /// The form drawn alone in a window, as the popover of a lane is (QUERY-028): no box of the host, a sort, real pointer and key events.
    struct Host {
        ctx: egui::Context,
        t: f64,
        st: State,
        text: String,
        sort: String,
        cache: Cache,
        advanced: bool,
        host_box: bool,
        vault: Vault,
        with_sort: bool,
        count: Option<String>,
        /// what the frames reported, since the last `take_changed`
        changed: Changed,
        nodes: Vec<(Role, Option<String>, Rect, bool)>,
    }

    impl Host {
        fn new(text: &str) -> Host {
            let ctx = egui::Context::default();
            crate::theme::apply(&ctx);
            ctx.enable_accesskit();
            let dir = std::env::temp_dir().join(format!("shale-form-{}-{}", std::process::id(), TEST_VAULTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
            std::fs::create_dir_all(&dir).unwrap();
            let vault = Vault::open(&dir).unwrap();
            let mut h =
                Host { vault, ctx, t: 0.0, st: State::default(), text: text.into(), sort: String::new(), cache: Cache::default(), advanced: false, host_box: false, with_sort: false, count: None, changed: Changed::default(), nodes: vec![] };
            h.frames(3);
            h
        }

        fn with_sort(text: &str, sort: &str) -> Host {
            let mut h = Host::new(text);
            (h.with_sort, h.sort) = (true, sort.into());
            h.frames(3);
            h
        }

        fn frame(&mut self, events: Vec<Event>) {
            self.t += 0.05;
            let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(520.0, 700.0))), time: Some(self.t), events, ..Default::default() };
            let Host { ctx, st, text, sort, cache, advanced, host_box, with_sort, count, changed, vault, .. } = self;
            let mut out = ctx.run_ui(raw, |ui| {
                let opts = Opts { vault, prefixes: Prefixes::default(), host_box: *host_box, count: count.as_deref() };
                let c = show(ui, st, text, cache, advanced, with_sort.then_some(sort), &opts);
                changed.query |= c.query;
                changed.sort |= c.sort;
                changed.advanced |= c.advanced;
            });
            out.textures_delta.clear();
            let tree = out.platform_output.accesskit_update.take().expect("accessibility is on");
            self.nodes = tree
                .nodes
                .iter()
                .filter_map(|(_, n)| {
                    let b = n.bounds()?;
                    Some((n.role(), n.label().or_else(|| n.value()).map(str::to_string), Rect::from_min_max(Pos2::new(b.x0 as f32, b.y0 as f32), Pos2::new(b.x1 as f32, b.y1 as f32)), n.is_disabled()))
                })
                .collect();
        }

        fn frames(&mut self, n: usize) {
            for _ in 0..n {
                self.frame(vec![]);
            }
        }

        /// The places of the widgets with this label, top to bottom.
        fn all(&self, label: &str) -> Vec<Rect> {
            let mut v: Vec<Rect> = self.nodes.iter().filter(|n| n.1.as_deref() == Some(label)).map(|n| n.2).collect();
            v.sort_by(|a, b| a.min.y.total_cmp(&b.min.y).then(a.min.x.total_cmp(&b.min.x)));
            v
        }

        fn find(&self, label: &str) -> Rect {
            self.all(label).first().copied().unwrap_or_else(|| panic!("no {label:?} on screen: {:?}", self.nodes.iter().filter_map(|n| n.1.clone()).collect::<Vec<_>>()))
        }

        fn has(&self, label: &str) -> bool {
            !self.all(label).is_empty()
        }

        /// Is this text on screen, as the whole label or a part of it?
        fn says(&self, text: &str) -> bool {
            self.nodes.iter().any(|n| n.1.as_deref().is_some_and(|l| l.contains(text)))
        }

        fn disabled(&self, label: &str) -> bool {
            self.nodes.iter().any(|n| n.1.as_deref() == Some(label) && n.3)
        }

        /// The text fields, top to bottom.
        fn edits(&self) -> Vec<Rect> {
            let mut v: Vec<Rect> = self.nodes.iter().filter(|n| n.0 == Role::TextInput).map(|n| n.2).collect();
            v.sort_by(|a, b| a.min.y.total_cmp(&b.min.y).then(a.min.x.total_cmp(&b.min.x)));
            v
        }

        fn click_at(&mut self, p: Pos2) {
            self.frame(vec![Event::PointerMoved(p)]);
            self.frame(vec![Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }]);
            self.frame(vec![Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }]);
            self.frames(3);
        }

        fn click(&mut self, label: &str) {
            let r = self.find(label);
            self.click_at(r.center());
        }

        /// The first, second, … widget with this label.
        fn click_nth(&mut self, label: &str, n: usize) {
            let r = self.all(label)[n];
            self.click_at(r.center());
        }

        /// Pick an item of a menu button.
        fn pick(&mut self, menu: &str, item: &str) {
            self.click(menu);
            self.click(item);
        }

        fn key(&mut self, key: Key, modifiers: Modifiers) {
            for pressed in [true, false] {
                self.frame(vec![Event::Key { key, physical_key: None, pressed, repeat: false, modifiers }]);
            }
            self.frames(2);
        }

        /// Click into the n-th text field and type `text` in place of what it holds.
        fn type_into(&mut self, n: usize, text: &str) {
            let r = self.edits()[n];
            self.click_at(r.center());
            self.key(Key::A, Modifiers::COMMAND);
            if text.is_empty() {
                self.key(Key::Backspace, Modifiers::NONE);
            } else {
                self.frame(vec![Event::Text(text.to_string())]);
                self.frames(2);
            }
        }

        /// A note of the vault, as the index has it.
        fn note(&mut self, name: &str, text: &str) {
            self.vault.set_note_text(std::path::Path::new(name), text);
            self.frames(2);
        }

        /// What the open list shows, top to bottom (none: no list).
        fn listed(&self) -> Vec<String> {
            self.st.complete.shown_items().map(<[String]>::to_vec).unwrap_or_default()
        }

        fn take_changed(&mut self) -> Changed {
            std::mem::take(&mut self.changed)
        }
    }

    impl Drop for Host {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.vault.root);
        }
    }

    // spec: QUERY-021
    #[test]
    fn query_021_the_menu_adds_each_kind_of_row_and_a_row_has_a_not_switch() {
        let mut h = Host::new("");
        assert!(!h.has("tag is"), "no rows in a blank query");
        for what in What::blanks() {
            h.pick("+ Add filter", what.label());
            let added = h.st.rows.last().unwrap();
            assert_eq!(std::mem::discriminant(&added.what), std::mem::discriminant(&what), "{}", what.label());
            assert!(!added.not);
        }
        assert_eq!(h.st.rows.len(), 8);
        assert_eq!(h.text, "due:<=+7d AND blocked AND blocking", "only the rows that are complete are in the text");
        for label in ["tag is", "property is", "due", "is blocked", "is blocking", "in folder", "links to", "text contains"] {
            assert!(h.has(label), "{label} is a row");
        }
        assert_eq!(h.all("not").len(), 8, "every row has a not switch");
        h.click_nth("not", 3);
        assert!(h.st.rows[3].not);
        assert_eq!(h.text, "due:<=+7d AND NOT blocked AND blocking");
        h.click_nth("not", 3);
        assert_eq!(h.text, "due:<=+7d AND blocked AND blocking", "the switch turns back");
        h.click_nth("not", 2);
        assert_eq!(h.text, "NOT due:<=+7d AND blocked AND blocking");
    }

    // spec: QUERY-021
    #[test]
    fn query_021_the_value_fields_write_the_text_and_a_quote_is_never_typed() {
        let mut h = Host::new("");
        h.pick("+ Add filter", "tag is");
        h.type_into(0, "todo");
        assert_eq!(h.text, "#todo");
        assert!(h.take_changed().query);
        h.pick("+ Add filter", "property is");
        // (the fields of the rows, top to bottom: the tag, the name and the value)
        h.type_into(1, "status");
        h.type_into(2, "in \"progress\"");
        assert_eq!(h.text, "#todo AND status:\"in progress\"", "the quotes typed are left out, white space is quoted");
        h.pick("+ Add filter", "text contains");
        h.type_into(3, "milk run");
        assert_eq!(h.text, "#todo AND status:\"in progress\" AND \"milk run\"");
        h.pick("+ Add filter", "in folder");
        h.type_into(4, "Projects/");
        h.pick("+ Add filter", "links to");
        h.type_into(5, "Big Plan");
        assert_eq!(h.text, "#todo AND status:\"in progress\" AND \"milk run\" AND path:Projects/ AND links:[[Big Plan]]");
        // an empty field is no filter, and the row says what it needs
        h.type_into(0, "");
        assert_eq!(h.text, "status:\"in progress\" AND \"milk run\" AND path:Projects/ AND links:[[Big Plan]]");
        assert!(h.says("a tag is a word"), "the empty row says why it is not in the query");
    }

    // spec: QUERY-021
    #[test]
    fn query_021_the_due_row_compares_with_a_day_or_asks_for_overdue() {
        let mut h = Host::new("");
        h.pick("+ Add filter", "due");
        assert_eq!(h.text, "due:<=+7d", "within 7 days to start with");
        h.type_into(0, "2026-12-01");
        assert_eq!(h.text, "due:<=2026-12-01");
        h.pick("within", "before");
        assert_eq!(h.text, "due:<2026-12-01");
        h.pick("before", "overdue");
        assert_eq!(h.text, "due:overdue");
        assert!(h.edits().is_empty(), "overdue has no day");
        assert_eq!(h.st.rows[0].what, What::Due { cmp: DueCmp::Overdue, day: "2026-12-01".into() }, "the day is kept for when the comparison comes back");
    }

    // spec: QUERY-022
    #[test]
    fn query_022_all_of_and_any_of_join_the_rows_and_come_back_from_the_text() {
        let mut h = Host::new("#a AND NOT status:done");
        assert_eq!(h.st.rows().0, Mode::All);
        h.click("Any of");
        assert_eq!(h.text, "#a OR NOT status:done");
        assert_eq!(h.st.rows().0, Mode::Any);
        h.click("All of");
        assert_eq!(h.text, "#a AND NOT status:done");
        // a text that is changed from outside is read again: OR is Any of
        h.text = "#a OR #b".into();
        h.frames(2);
        assert_eq!(h.st.rows().0, Mode::Any);
        assert_eq!(h.st.rows().1.len(), 2);
        // one row has no mode: the choice stays
        h.text = "#a".into();
        h.frames(2);
        assert_eq!(h.st.rows().0, Mode::Any);
        // a row removed with its ✕ leaves the others
        h.text = "#a OR #b OR #c".into();
        h.frames(2);
        h.click_nth("✕", 1);
        assert_eq!(h.text, "#a OR #c");
    }

    // spec: QUERY-023
    #[test]
    fn query_023_sort_rows_are_added_turned_reordered_and_removed() {
        let mut h = Host::with_sort("", "");
        assert!(h.has("Sort"));
        h.pick("+ Add sort", "due");
        h.pick("+ Add sort", "priority");
        assert_eq!(h.sort, "due asc, priority asc");
        assert!(h.take_changed().sort);
        // asc turns to desc
        h.click_nth("asc", 1);
        assert_eq!(h.sort, "due asc, priority desc");
        // the field of a row is chosen among the fields that no other row has
        h.click("due");
        assert!(!h.all("priority").is_empty() && h.all("title").len() == 1 && !h.all("due").is_empty());
        h.click("title");
        assert_eq!(h.sort, "title asc, priority desc");
        // ▼ and ▲ move a row; the first has no ▲ and the last no ▼
        h.click_nth("▼", 0);
        assert_eq!(h.sort, "priority desc, title asc");
        h.click_nth("▲", 1);
        assert_eq!(h.sort, "title asc, priority desc");
        // dependencies has no direction
        h.pick("+ Add sort", "dependencies");
        assert_eq!(h.sort, "title asc, priority desc, dependencies");
        assert_eq!(h.all("asc").len() + h.all("desc").len(), 2, "the third row has no direction");
        // remove the first
        h.click_nth("✕", 0);
        assert_eq!(h.sort, "priority desc, dependencies");
        assert_eq!(h.st.sort.unused().len(), 4);
        // and the form left the query alone
        assert_eq!(h.text, "");
    }

    // spec: QUERY-023
    #[test]
    fn query_023_the_first_row_cannot_go_up_and_the_last_cannot_go_down() {
        let h = Host::with_sort("", "due asc");
        assert!(h.disabled("▲") && h.disabled("▼"));
        let h = Host::with_sort("", "due asc, title asc");
        assert!(h.all("▲").len() == 2 && h.all("▼").len() == 2);
    }

    // spec: QUERY-023
    #[test]
    fn query_023_a_sort_row_is_reordered_by_dragging_its_handle() {
        let mut h = Host::with_sort("", "due asc, priority desc, title asc");
        let handles = h.all("Drag to reorder");
        assert_eq!(handles.len(), 3, "{:?}", h.nodes.iter().map(|n| (n.0, n.1.clone())).collect::<Vec<_>>());
        let (from, to) = (handles[0].center(), handles[2].center());
        h.frame(vec![Event::PointerMoved(from)]);
        h.frame(vec![Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }]);
        let steps = 6;
        for k in 1..=steps {
            h.frame(vec![Event::PointerMoved(from + (to - from) * (k as f32 / steps as f32))]);
        }
        h.frame(vec![Event::PointerButton { pos: to, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }]);
        h.frames(3);
        assert_eq!(h.sort, "priority desc, title asc, due asc", "the first row went to the place of the last");
    }

    // spec: QUERY-023
    #[test]
    fn query_023_a_sort_that_is_not_understood_stays_as_text() {
        let mut h = Host::with_sort("", "colour, due");
        assert!(h.has("Sort") && !h.has("+ Add sort"), "no rows for a sort that is not read");
        assert_eq!(h.edits().len(), 1, "its text is edited instead");
        assert!(h.says("not a sort field"), "and says why");
        assert_eq!(h.sort, "colour, due", "nothing was dropped");
        h.type_into(0, "title desc");
        assert_eq!(h.sort, "title desc");
        assert!(h.take_changed().sort);
        assert!(h.has("+ Add sort"), "read again: the rows are back");
    }

    // spec: QUERY-024
    #[test]
    fn query_024_advanced_swaps_the_form_for_the_text_with_its_errors_and_the_count() {
        let mut h = Host::new("#todo AND status:doing");
        h.count = Some("3 matches".into());
        h.frames(2);
        assert!(h.has("All of") && !h.says("3 matches"), "the count is for Advanced");
        h.click("Advanced");
        assert!(h.advanced && h.take_changed().advanced);
        assert!(!h.has("All of") && !h.has("+ Add filter"), "no rows in Advanced");
        assert_eq!(h.edits().len(), 1, "the text box");
        assert!(h.says("3 matches"), "the live count");
        // the text is edited; the error is said under it
        h.type_into(0, "#todo AND (");
        assert_eq!(h.text, "#todo AND (");
        assert!(h.take_changed().query);
        let reason = h.cache.get(&h.text, Prefixes::default()).error().expect("the text has an error").reason.clone();
        assert!(h.says(&reason), "the reason of the error is under the text: {reason}");
        // back to the form with a text that fits: the rows follow the text
        h.text = "#a OR #b".into();
        h.click("Advanced");
        assert!(!h.advanced);
        assert!(h.take_changed().advanced);
        assert_eq!(h.st.rows().0, Mode::Any);
        assert_eq!(h.st.rows().1.len(), 2);
    }

    // spec: QUERY-024
    #[test]
    fn query_024_a_host_with_its_own_box_gets_no_second_one_and_shows_the_error_itself() {
        let mut h = Host::new("#a AND (");
        h.host_box = true;
        h.advanced = true;
        h.count = Some("2 matches".into());
        h.frames(2);
        assert!(h.edits().is_empty() && h.says("Edit the query in the box above.") && h.says("2 matches"));
        assert!(!h.says("not closed"), "the host says what is wrong with the text");
    }

    // spec: QUERY-025
    #[test]
    fn query_025_a_query_that_does_not_fit_stays_as_it_is_and_advanced_stays_on() {
        let complex = "(#a OR #b) AND NOT (status:done OR \"x y\")";
        let mut h = Host::new(complex);
        assert_eq!(h.st.fit, Fit::Complex);
        assert!(h.says("This query is too complex for the form"));
        assert!(!h.advanced, "the setting is not touched");
        assert!(!h.has("All of") && !h.has("+ Add filter") && h.has("Advanced"));
        assert!(h.disabled("Advanced"), "Advanced cannot be turned off");
        h.click("Advanced");
        assert!(!h.advanced && !h.take_changed().advanced);
        assert_eq!(h.text, complex, "nothing was simplified or dropped");
        // the text is there to edit, and a text that fits brings the form back
        assert_eq!(h.edits().len(), 1);
        h.text = "#a".into();
        h.frames(2);
        assert!(h.has("All of") && !h.says("too complex"));
        // a text with an error is not shown by the form either, and the form does not write over it
        h.text = "#a AND (".into();
        h.frames(2);
        assert!(!h.has("All of") && h.says("Fix the text first"));
        assert_eq!(h.text, "#a AND (");
    }

    // spec: QUERY-025
    #[test]
    fn query_025_a_prefix_stays_in_front_of_what_the_form_writes() {
        let mut h = Host::new("m: #todo");
        assert_eq!(h.st.rows().1.len(), 1);
        h.pick("+ Add filter", "is blocked");
        assert_eq!(h.text, "m: #todo AND blocked");
    }

    // spec: QUERY-021
    #[test]
    fn query_021_the_form_is_reached_with_the_keyboard() {
        let mut h = Host::new("#a");
        h.st.focus();
        h.frames(2);
        let first = h.ctx.memory(|m| m.focused());
        assert!(first.is_some(), "the first widget took the keyboard");
        // Tab walks on (All of, Any of, …); Enter on the focused Any of chooses it
        h.key(Key::Tab, Modifiers::NONE);
        assert_ne!(h.ctx.memory(|m| m.focused()), first);
        h.key(Key::Enter, Modifiers::NONE);
        assert_eq!(h.st.rows().0, Mode::Any, "Enter on the focused Any of");
        let mut seen = std::collections::HashSet::new();
        for _ in 0..12 {
            h.key(Key::Tab, Modifiers::NONE);
            seen.extend(h.ctx.memory(|m| m.focused()));
        }
        assert!(seen.len() >= 5, "Tab reaches the row's switch, field and remove button, the menu and Advanced: {}", seen.len());
    }

    #[test]
    fn the_sort_fields_are_the_six_of_the_spec() {
        assert_eq!(Field::ALL.map(Field::word), ["created", "modified", "due", "priority", "title", "dependencies"]);
        assert_eq!(sort::write(&[]), "");
    }

    fn vault_host() -> Host {
        let mut h = Host::new("");
        h.note("a.md", "---\nstatus: Doing\nowner: zoe\ncreated: 2026-01-01 10:00\nmodified: 2026-02-02 11:00\ntags: [x]\n---\n#project/shale #Bug\n");
        h.note("b.md", "---\nstatus: todo\nowner: Adam\n---\n#alpha #shale-ui\n");
        h.note("c.md", "---\nstatus: done\npeople:\n  - bob\n  - al\n---\n#beta\n");
        h
    }

    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    // spec: QUERY-029
    #[test]
    fn query_029_an_empty_tag_field_lists_every_tag_of_the_vault_alphabetically() {
        let mut h = vault_host();
        h.pick("+ Add filter", "tag is");
        assert!(h.listed().is_empty(), "no list until the field has the keyboard");
        let r = h.edits()[0];
        h.click_at(r.center());
        assert_eq!(h.listed(), strings(&["alpha", "beta", "Bug", "project/shale", "shale-ui", "x"]), "all, without regard to case");
        assert!(list_open(&h.ctx));
        h.vault.set_note_text(std::path::Path::new("d.md"), "#Zed #apple");
        h.frames(2);
        assert_eq!(h.listed().first().map(String::as_str), Some("alpha"), "the vocabulary follows the vault");
        assert!(h.listed().contains(&"Zed".to_string()) && h.listed().contains(&"apple".to_string()), "{:?}", h.listed());
    }

    // spec: QUERY-029
    #[test]
    fn query_029_typing_narrows_prefix_matches_first_then_inside_matches_each_alphabetical() {
        let mut h = vault_host();
        h.pick("+ Add filter", "tag is");
        h.type_into(0, "S");
        assert_eq!(h.listed(), strings(&["shale-ui", "project/shale"]), "a prefix before an inside match, any case");
        h.type_into(0, "b");
        assert_eq!(h.listed(), strings(&["beta", "Bug"]));
        h.type_into(0, "zzz");
        assert!(h.listed().is_empty(), "nothing fits");
    }

    // spec: QUERY-029
    #[test]
    fn query_029_the_arrow_keys_choose_and_enter_tab_and_a_click_take_an_entry_into_the_query() {
        let mut h = vault_host();
        h.pick("+ Add filter", "tag is");
        let r = h.edits()[0];
        h.click_at(r.center());
        h.key(Key::ArrowDown, Modifiers::NONE);
        h.key(Key::ArrowDown, Modifiers::NONE);
        h.key(Key::Enter, Modifiers::NONE);
        assert_eq!(h.text, "#beta", "down twice, Enter");
        assert!(h.listed().is_empty(), "taking closes the list");
        assert!(h.take_changed().query);
        h.type_into(0, "pro");
        assert_eq!(h.listed(), strings(&["project/shale"]));
        h.key(Key::Tab, Modifiers::NONE);
        assert_eq!(h.text, "#project/shale", "the first entry is chosen while typing; Tab takes it");
        h.type_into(0, "");
        h.key(Key::ArrowUp, Modifiers::NONE);
        h.key(Key::Enter, Modifiers::NONE);
        assert_eq!(h.text, "#x", "up from nothing is the last entry");
        h.type_into(0, "");
        h.click("Bug");
        assert_eq!(h.text, "#Bug", "a click takes the entry");
        assert!(h.listed().is_empty());
        h.type_into(0, "");
        h.key(Key::J, Modifiers::CTRL);
        h.key(Key::Enter, Modifiers::NONE);
        assert_eq!(h.text, "#alpha", "Ctrl+J is down, as in the other lists");
    }

    // spec: QUERY-029
    #[test]
    fn query_029_tab_through_an_empty_field_takes_nothing_and_esc_closes_only_the_list() {
        let mut h = vault_host();
        h.pick("+ Add filter", "tag is");
        let r = h.edits()[0];
        h.click_at(r.center());
        h.key(Key::Tab, Modifiers::NONE);
        assert_eq!(h.text, "", "nothing was chosen, nothing is taken");
        h.click_at(r.center());
        assert!(!h.listed().is_empty());
        h.key(Key::Escape, Modifiers::NONE);
        assert!(h.listed().is_empty(), "Esc closes the list");
        assert!(!list_open(&h.ctx));
        h.type_into(0, "b");
        assert!(!h.listed().is_empty(), "typing opens it again");
    }

    // spec: QUERY-029
    #[test]
    fn query_029_a_property_lists_the_names_and_then_the_values_of_that_name() {
        let mut h = vault_host();
        h.pick("+ Add filter", "property is");
        let name = h.edits()[0];
        h.click_at(name.center());
        assert_eq!(h.listed(), strings(&["owner", "people", "status", "tags"]), "the names, without Shale's stamps created and modified");
        h.type_into(0, "st");
        h.key(Key::Enter, Modifiers::NONE);
        assert_eq!(h.st.rows[0].what, What::Prop { key: "status".into(), value: String::new() });
        let value = h.edits()[1];
        h.click_at(value.center());
        assert_eq!(h.listed(), strings(&["Doing", "done", "todo"]), "the values of status, once each, any case");
        h.type_into(1, "do");
        assert_eq!(h.listed(), strings(&["Doing", "done", "todo"]), "todo has it inside");
        h.key(Key::ArrowDown, Modifiers::NONE);
        h.key(Key::Enter, Modifiers::NONE);
        assert_eq!(h.text, "status:done");
        h.type_into(0, "people");
        h.type_into(1, "");
        assert_eq!(h.listed(), strings(&["al", "bob"]), "a list gives an entry for each item");
        h.type_into(0, "created");
        h.type_into(1, "");
        assert!(h.listed().is_empty(), "a time stamp is no value to pick");
        h.type_into(0, "owner");
        h.type_into(1, "");
        assert_eq!(h.listed(), strings(&["Adam", "zoe"]));
    }
}
