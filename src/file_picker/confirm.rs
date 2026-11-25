//! The step after the system's dialog has chosen a folder for a request with a box of its own
//! (Create…: "Start with sample notes", VAULT-034, PICK-011): the system's dialog cannot hold the
//! box, so a small dialog of Shale's shows the folder that was chosen, the box, and the button
//! and Cancel. Shale's own picker has the box in the dialog itself, so both offer the same choice.

use super::own::Step;
use super::{Picked, Request};
use eframe::egui::{self, Context, Key, Ui};
use std::path::PathBuf;

pub struct Confirm {
    req: Request,
    paths: Vec<PathBuf>,
    pub(super) option: bool,
}

/// What the person pressed in a frame.
enum Asked {
    Choose,
    Cancel,
}

impl Confirm {
    pub fn new(req: Request, paths: Vec<PathBuf>) -> Confirm {
        Confirm { option: req.option_on, req, paths }
    }

    /// Draw the dialog; Enter and the button choose, Esc and Cancel close it without an answer.
    pub fn show(&mut self, ctx: &Context) -> Step {
        let title = self.req.title.clone();
        let mut st = (self, None::<Asked>);
        let resp = crate::dialog::Dialog::new("file-picker-confirm", title).width(480.0).show_with_footer(ctx, &mut st, |ui, (c, asked)| c.body(ui, asked), |ui, (c, asked)| c.footer(ui, asked));
        let (c, asked) = st;
        match asked {
            Some(Asked::Choose) => Step::Done(Picked { paths: c.paths.clone(), option: c.option, offered: false }),
            Some(Asked::Cancel) => Step::Cancel,
            None if resp.should_close() => Step::Cancel,
            None => Step::Keep,
        }
    }

    fn body(&mut self, ui: &mut Ui, asked: &mut Option<Asked>) {
        crate::theme::hint(ui, "Folder");
        let shown = self.paths.first().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        crate::theme::paragraph(ui, shown);
        ui.add_space(6.0);
        if let Some(label) = self.req.option.clone() {
            crate::clickable::pointer(ui.checkbox(&mut self.option, label));
        }
        if let Some(h) = &self.req.hint {
            crate::theme::hint(ui, h.as_str());
        }
        if ui.input(|i| i.key_pressed(Key::Enter)) {
            *asked = Some(Asked::Choose);
        }
    }

    fn footer(&mut self, ui: &mut Ui, asked: &mut Option<Asked>) {
        ui.horizontal(|ui| {
            if ui.button(egui::RichText::new(self.req.button.as_str()).strong()).clicked() {
                *asked = Some(Asked::Choose);
            }
            if ui.button("Cancel").clicked() {
                *asked = Some(Asked::Cancel);
            }
        });
    }
}
