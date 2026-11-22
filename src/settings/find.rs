//! The search field at the top of the Settings dialog (CFG-072, CFG-073): typing lists the
//! settings and key actions that fit (`registry::search`), ↓ / ↑ move through the list, Enter or
//! a click goes to the setting: its tab is shown, the page scrolls to it and a box pulses around
//! it (`anchor`). Ctrl+F, or `/` while no text field has the keyboard, focuses the field.

use super::anchor;
use super::registry::{self, Hit};
use super::SettingsTab;
use crate::keys::Keymap;
use crate::keys_ui::KeyDialog;
use crate::picker::ListKeys;
use crate::theme;
use eframe::egui::{self, text::LayoutJob, Event, Id, Key, Modifiers, Popup, PopupCloseBehavior, RichText, TextFormat, Ui};

/// How many results the list shows before it scrolls.
const VISIBLE_ROWS: f32 = 9.0;

/// The state of the field and its list of results.
#[derive(Default)]
pub struct Find {
    query: String,
    /// The result the keys are on.
    sel: usize,
    /// The list is shown.
    open: bool,
    /// The field takes the keyboard in the next frame.
    focus: bool,
}

fn field_id() -> Id {
    Id::unique("settings-find-field")
}

/// Take Ctrl+F, and `/` when no text field has the keyboard: they mean "search" (the text of a
/// `/` that was typed is taken with it). Returns whether one was pressed.
pub fn take_search_keys(ui: &Ui) -> bool {
    let typing = ui.ctx().egui_wants_keyboard_input();
    ui.input_mut(|i| {
        let ctrl_f = i.consume_key(Modifiers::COMMAND, Key::F);
        let slash = !typing && i.consume_key(Modifiers::NONE, Key::Slash);
        if slash {
            i.events.retain(|e| !matches!(e, Event::Text(t) if t == "/"));
        }
        ctrl_f || slash
    })
}

/// Esc in a text field takes its focus away unless the field says it wants the key. The fields of
/// the dialog do: Esc is theirs first (it closes the list, or clears the text) and the dialog's
/// after. Call it after the field is added, as the field sets its own filter.
pub fn keep_focus_on_esc(ctx: &egui::Context, id: Id) {
    ctx.memory_mut(|m| m.set_focus_lock_filter(id, egui::EventFilter { escape: true, horizontal_arrows: true, vertical_arrows: true, tab: false }));
}

impl Find {
    #[cfg(test)]
    pub fn query(&self) -> &str {
        &self.query
    }

    /// The field and, below it while there is a query, the list of results. A result that is picked
    /// shows its tab (`tab`) and starts the jump to its setting. `locked`: a binding is being
    /// recorded or asked about, and the keys belong to that.
    pub fn show(&mut self, ui: &mut Ui, keymap: &Keymap, tab: &mut SettingsTab, keys: &mut KeyDialog, locked: bool) {
        let ctx = ui.ctx().clone();
        if !locked && *tab != SettingsTab::Keybindings && take_search_keys(ui) {
            self.focus = true;
        }
        let focused = ctx.memory(|m| m.has_focus(field_id()));
        let mut hits = registry::search(&self.query, keymap);
        self.sel = self.sel.min(hits.len().saturating_sub(1));
        // the keys of the list are taken before the field sees them
        let (mut picked, mut moved) = (None, false);
        if !locked {
            if self.open {
                let list = ListKeys::take(ui, keymap);
                let before = self.sel;
                list.select(&mut self.sel, hits.len());
                moved = self.sel != before;
                if list.enter {
                    picked = hits.get(self.sel).cloned();
                }
                if list.esc {
                    self.open = false;
                }
            } else if focused && !self.query.trim().is_empty() && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowDown)) {
                self.open = true;
            }
        }
        let field = ui.add_enabled(!locked, egui::TextEdit::singleline(&mut self.query).id(field_id()).hint_text("Search all settings and keys…  (Ctrl+F)").desired_width(f32::INFINITY));
        keep_focus_on_esc(&ctx, field_id());
        if std::mem::take(&mut self.focus) {
            field.request_focus();
        }
        if field.changed() {
            self.sel = 0;
            self.open = true;
            hits = registry::search(&self.query, keymap);
        }
        let mut open = self.open && !locked && !self.query.trim().is_empty();
        let mut sel = self.sel;
        let hint = ListKeys::hint(keymap);
        Popup::from_response(&field).id(Id::unique("settings-find-popup")).open_bool(&mut open).close_behavior(PopupCloseBehavior::CloseOnClickOutside).width(field.rect.width()).show(|ui| {
            ui.set_min_width(field.rect.width() - 2.0 * ui.spacing().window_margin.leftf());
            if hits.is_empty() {
                ui.label(RichText::new("No setting matches").color(theme::palette().muted));
            }
            let row_h = ui.text_style_height(&egui::TextStyle::Button) + ui.spacing().item_spacing.y + ui.spacing().button_padding.y * 2.0;
            egui::ScrollArea::vertical().id_salt("settings-find-list").max_height(row_h * VISIBLE_ROWS).auto_shrink([false, true]).show(ui, |ui| {
                ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                    for (i, hit) in hits.iter().enumerate() {
                        let row = ui.add(egui::Button::selectable(i == sel, row_label(ui, hit)).right_text(RichText::new(hit.tab.label()).color(theme::palette().muted)));
                        if moved && i == sel {
                            row.scroll_to_me(None);
                        }
                        if row.clicked() {
                            picked = Some(hit.clone());
                        } else if row.hovered() && ui.input(|inp| inp.pointer.delta() != egui::Vec2::ZERO) {
                            sel = i;
                        }
                    }
                });
            });
            if !hint.is_empty() {
                theme::hint(ui, hint.clone());
            }
        });
        self.sel = sel;
        // a click in the field is outside the list, but it is where the person is typing
        self.open = open || field.clicked() || (field.gained_focus() && !self.query.trim().is_empty());
        if focused && !field.has_focus() && !ctx.input(|i| i.pointer.any_down()) {
            // the keyboard went elsewhere (Tab), not to a click on the list
            self.open = false;
        }
        if let Some(hit) = picked {
            self.pick(&ctx, &hit, tab, keys, keymap);
        }
    }

    /// Go to a result: show its tab, scroll to it and pulse a box around it.
    pub fn pick(&mut self, ctx: &egui::Context, hit: &Hit, tab: &mut SettingsTab, keys: &mut KeyDialog, keymap: &Keymap) {
        *tab = hit.tab;
        if let Some(action) = hit.action {
            keys.reveal(action, keymap);
        }
        anchor::start(ctx, &hit.id);
        self.open = false;
    }
}

/// The name of a result, and for an action the keys it is bound to.
fn row_label(ui: &Ui, hit: &Hit) -> LayoutJob {
    let mut job = LayoutJob::default();
    let font = egui::TextStyle::Button.resolve(ui.style());
    let plain = TextFormat { font_id: font.clone(), color: ui.visuals().text_color(), ..Default::default() };
    job.append(hit.label, 0.0, plain);
    if let Some(keys) = &hit.keys {
        job.append(&format!("   {keys}"), 0.0, TextFormat { font_id: font, color: theme::palette().accent, ..Default::default() });
    }
    job
}
