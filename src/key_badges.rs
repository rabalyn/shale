//! The badges that show the Alt key of a button while Alt is held (KEY-016): a small label with the
//! key's text (`B`, `1`, `,`) at the corner of each shown, enabled button whose action has a
//! single-chord Alt key, following a rebound key. The mail view's buttons underline a letter instead
//! (MAIL-229); both read the key through `Keymap::alt_chord`.

use crate::keys::{Action, Keymap, DEFS};
use crate::theme;
use eframe::egui::{Align2, Color32, CornerRadius, FontId, Id, LayerId, Order, Pos2, Rect, Stroke, StrokeKind, Ui, Vec2};
use std::collections::HashMap;

/// The size of a badge's text; a test finds the badges by it.
pub const FONT_SIZE: f32 = 9.5;

/// The badge texts of one frame: empty unless Alt is held without Ctrl (so AltGr, which Windows
/// reports as Ctrl+Alt, shows none).
#[derive(Default)]
pub struct Badges(HashMap<Action, String>);

impl Badges {
    pub fn of_frame(keys: &Keymap, ui: &Ui) -> Badges {
        let m = ui.input(|i| i.modifiers);
        if !m.alt || m.ctrl || m.command {
            return Badges::default();
        }
        Badges(DEFS.iter().filter_map(|d| keys.alt_badge(d.action).map(|t| (d.action, t))).collect())
    }

    /// Draw the badge of `a` at the bottom right corner of `rect`, when it has one.
    pub fn paint(&self, ui: &Ui, rect: Rect, a: Action) {
        let Some(text) = self.0.get(&a) else { return };
        // on top of the neighbouring buttons and of what clips the bar
        let painter = ui.ctx().layer_painter(LayerId::new(Order::Tooltip, Id::unique("key-badges")));
        let galley = painter.layout_no_wrap(text.clone(), FontId::proportional(FONT_SIZE), theme::palette().text_strong);
        let size = galley.size() + Vec2::new(6.0, 2.0);
        let badge = Rect::from_min_size(Pos2::new(rect.right() - size.x + 2.0, rect.bottom() - size.y + 2.0), size);
        painter.rect(badge, CornerRadius::same(3), theme::palette().bg_active, Stroke::new(1.0, theme::palette().accent), StrokeKind::Inside);
        painter.galley(Align2::CENTER_CENTER.align_size_within_rect(galley.size(), badge).min, galley, Color32::PLACEHOLDER);
    }
}
