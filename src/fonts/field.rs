//! The font fields of the exports (PDF-039, CFG-080): a text field that takes a family name or a
//! vault path and offers the families the scan found in a list, and the line that says what a
//! value comes to (found where, or why it cannot be used). The export dialog and Settings share them.

use super::resolve::{self, Chosen, Engine, Request, Role, Values};
use super::scan::{name_key, Catalogue};
use crate::clickable::{self, ComboBoxPointer};
use eframe::egui::{self, Response, Ui};
use std::path::Path;
use std::sync::Arc;

/// How many families the list shows at once; the rest is reached by typing.
const SHOWN: usize = 60;

/// The families the scan found, for the list of a field.
#[derive(Clone, Debug, Default)]
pub struct FamilyList {
    pub names: Arc<Vec<String>>,
    /// The system fonts are still being looked up.
    pub pending: bool,
}

/// The family names of a catalogue, each once (ignoring case, spaces and dashes), in alphabetical order.
pub fn family_names(cat: &Catalogue) -> Vec<String> {
    let mut names: Vec<String> = cat.vault.iter().chain(cat.system.iter()).filter_map(|f| f.families.first()).cloned().collect();
    // of names that differ in case, spaces and dashes only, the one that sorts first stays
    names.sort_by_key(|n| (name_key(n), n.clone()));
    names.dedup_by_key(|n| name_key(n));
    names.sort_by_key(|n| n.to_lowercase());
    names
}

/// What a value of a font field comes to, in words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Check {
    pub text: String,
    /// The value cannot be used.
    pub problem: bool,
}

/// Whether a value of the vault's settings (CFG-080) is found and where from, or why it cannot be
/// used (PDF-037, PDF-038). None for an empty value. The scan of the installed fonts being out
/// still, a name the vault does not have is not known to be missing.
pub fn check(role: Role, value: &str, vault_root: &Path, cat: &Catalogue) -> Option<Check> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let mut vault = Values::default();
    match role {
        Role::Text => vault.text = Some(value.to_string()),
        Role::Sans => vault.sans = Some(value.to_string()),
        Role::Mono => vault.mono = Some(value.to_string()),
    }
    let request = Request { engine: Engine::Tex, vault_root, note: Values::default(), book: false, chapters: Vec::new(), set: None, vault };
    let resolved = resolve::resolve(&request, cat);
    if let Some(w) = resolved.warnings.first() {
        return Some(Check { text: w.clone(), problem: true });
    }
    let pick = resolved.pick(role);
    Some(match &pick.chosen {
        Chosen::TexLive(_) if !cat.system_ready => Check { text: "Looking up fonts...".into(), problem: false },
        Chosen::TexLive(name) => Check { text: format!("“{name}” is not installed and not in the vault: the built-in PDF will not find it; the LaTeX export asks TeX Live when exporting."), problem: false },
        Chosen::Bundled(_) => Check { text: format!("{} (built into the PDF; the LaTeX export keeps the template's font)", pick.describe()), problem: false },
        _ => Check { text: format!("Found: {}", pick.describe()), problem: false },
    })
}

/// A field for a font: the text, and beside it a list of the families (narrowed by what is typed)
/// to pick from. `id` names the field and its controls for the tests (`font:<id>`,
/// `font:<id>:list`, `font:<id>:pick:<family>`). True when the value changed.
pub fn font_field(ui: &mut Ui, id: &str, value: &mut String, hint: &str, list: &FamilyList, rec: &mut dyn FnMut(&str, Response) -> Response) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        let edit = rec(&format!("font:{id}"), ui.add(egui::TextEdit::singleline(value).hint_text(hint).desired_width(190.0)));
        changed |= edit.changed();
        let combo = egui::ComboBox::from_id_salt(("font-families", id)).selected_text("").width(0.0).show_pointer(ui, |ui| {
            ui.set_min_width(220.0);
            if list.names.is_empty() {
                ui.label(egui::RichText::new(if list.pending { "Looking up fonts..." } else { "No fonts found" }).weak());
                return;
            }
            let key = name_key(value);
            // what is typed narrows the list, unless it already is a family (the person wants another
            // one) or fits none (the whole list is the help there is)
            let narrow = !key.is_empty() && !list.names.iter().any(|n| name_key(n) == key) && list.names.iter().any(|n| name_key(n).contains(&key));
            let mut matching = list.names.iter().filter(|n| !narrow || name_key(n).contains(&key));
            egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                for name in matching.by_ref().take(SHOWN) {
                    if rec(&format!("font:{id}:pick:{name}"), clickable::pointer(ui.selectable_label(name_key(name) == key, name))).clicked() {
                        *value = name.clone();
                        changed = true;
                    }
                }
                let more = matching.count();
                if more > 0 {
                    ui.label(egui::RichText::new(format!("{more} more: type to narrow the list")).weak());
                }
            });
            if list.pending {
                ui.label(egui::RichText::new("Looking up fonts...").weak());
            }
        });
        rec(&format!("font:{id}:list"), combo.response);
    });
    changed
}
