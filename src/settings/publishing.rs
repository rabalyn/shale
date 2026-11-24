//! The Publishing tab (SITE-051, SITE-058, SITE-064, SITE-065, SITE-071, SITE-072): the vault's
//! switch for publishing it as a Hugo site, the rules that choose the notes, the address and branch
//! of the publish remote, and the Publish button. Everything is the vault's own setting
//! (`.shale/settings.json`, SITE-052); the page only draws it and starts the jobs of
//! `site_export/job.rs`.

use super::anchor::anchor;
use crate::app::ShaleApp;
use crate::busy;
use crate::gitsync::wizard::test_sync;
use crate::site_export::job::{Lists, PUBLISH, PUBLISH_LOOK};
use crate::site_export::publish::{self, local};
use crate::theme;
use crate::vault_state::PublishSettings;
use eframe::egui::{self, Ui};

/// The private note types that "all notes" can take in (SITE-056), as the page names them.
const PRIVATE: [(&str, &str); 4] = [("email", "e-mails"), ("event", "events"), ("post", "posts"), ("person", "contacts")];

/// A label and a few lines of text, one rule per line.
fn list_edit(ui: &mut Ui, label: &str, text: &mut String, hint: &str) -> bool {
    ui.label(label);
    ui.add(egui::TextEdit::multiline(text).desired_rows(2).desired_width(320.0).hint_text(hint)).changed()
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

impl ShaleApp {
    pub(super) fn publishing_tab(&mut self, ui: &mut Ui) -> bool {
        let Some(uid) = self.vault.as_ref().map(|v| v.uid) else {
            theme::hint(ui, "Open a vault to publish it.");
            return false;
        };
        if self.publishing.lists.vault != uid {
            self.publishing.lists = Lists::of(uid, &self.vdata.settings.publish);
            self.publishing.url_text = None;
            self.publishing.site_address_text = None;
            self.publishing.looked_once = false;
        }
        let mut changed = anchor(ui, "publishing.enabled", |ui| ui.checkbox(&mut self.vdata.settings.publish.enabled, "Publish this vault as a website").changed());
        theme::hint(ui, "Off by default. Only the notes chosen below, with the media they show, are written as a Hugo site and pushed to the address below as one snapshot; the rest of the vault never leaves this computer.");
        if !self.vdata.settings.publish.enabled {
            theme::hint(ui, "Publishing is off for this vault: no note is published.");
            return changed;
        }
        changed |= self.publishing_rules_ui(ui);
        changed |= self.publishing_target_ui(ui);
        changed |= self.publishing_site_address_ui(ui);
        self.publishing_run_ui(ui);
        theme::hint(
            ui,
            "A snapshot replaces the last one, so a note you stop publishing is gone from the branch and its history at the next publish. Media files that Git LFS stores stay on the server's LFS store until the server collects them. The user guide note \"Publishing a vault\" tells how to build the site on a server.",
        );
        changed
    }

    /// Which notes: all notes, folders, tags, the private types, and what they choose (SITE-053…SITE-058).
    fn publishing_rules_ui(&mut self, ui: &mut Ui) -> bool {
        theme::section(ui, "Which notes");
        let mut changed = false;
        let s = &mut self.vdata.settings.publish;
        changed |= anchor(ui, "publishing.all_notes", |ui| ui.checkbox(&mut s.all_notes, "All notes (not e-mails, events, posts and contacts, unless ticked below)").changed());
        anchor(ui, "publishing.private_types", |ui| {
            ui.add_enabled_ui(s.all_notes, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label("Also publish");
                    for (kind, label) in PRIVATE {
                        let mut on = s.private_types.iter().any(|t| crate::site_export::rules::kind_of(&t.trim().to_lowercase()) == kind);
                        if ui.checkbox(&mut on, label).changed() {
                            s.private_types.retain(|t| crate::site_export::rules::kind_of(&t.trim().to_lowercase()) != kind);
                            if on {
                                s.private_types.push(kind.to_string());
                            }
                            changed = true;
                        }
                    }
                });
            });
        });
        let lists = &mut self.publishing.lists;
        let mut edited = false;
        edited |= anchor(ui, "publishing.include", |ui| list_edit(ui, "Publish the notes in these folders (one per line)", &mut lists.include_folders, "Garden"));
        edited |= anchor(ui, "publishing.include_tags", |ui| list_edit(ui, "Publish the notes with these tags (one per line)", &mut lists.include_tags, "#public"));
        edited |= anchor(ui, "publishing.exclude", |ui| list_edit(ui, "Never publish the notes in these folders", &mut lists.exclude_folders, "Garden/Drafts"));
        edited |= anchor(ui, "publishing.exclude_tags", |ui| list_edit(ui, "Never publish the notes with these tags", &mut lists.exclude_tags, "#private"));
        if edited && self.publishing.lists.apply(&mut self.vdata.settings.publish) {
            changed = true;
        }
        theme::hint(ui, "A note's own property publish: true or publish: false wins over every rule. A rule of exclusion wins over a rule of inclusion.");
        self.publishing_count_ui(ui);
        changed
    }

    /// How many notes the rules choose, and the button that lists them (SITE-058).
    fn publishing_count_ui(&mut self, ui: &mut Ui) {
        let n = self.publish_chosen().len();
        anchor(ui, "publishing.count", |ui| {
            ui.horizontal(|ui| {
                ui.label(if n == 1 { "1 note is chosen by these rules.".to_string() } else { format!("{n} notes are chosen by these rules.") });
                let open = self.publishing.list_open;
                if ui.add_enabled(n > 0, egui::Button::new(if open { "Hide the list" } else { "List the notes" }).small()).clicked() {
                    self.publishing.list_open = !open;
                }
            });
        });
        if self.publishing.list_open && n > 0 {
            let notes: Vec<String> = self.publish_chosen().iter().map(|p| crate::vault::rel_to_string(p)).collect();
            egui::ScrollArea::vertical().id_salt("publishing-list").max_height(220.0).auto_shrink([false, true]).show(ui, |ui| {
                for note in &notes {
                    ui.label(note);
                }
            });
        }
    }

    /// Where: the address and branch (SITE-064).
    fn publishing_target_ui(&mut self, ui: &mut Ui) -> bool {
        theme::section(ui, "Where to publish");
        let mut changed = false;
        let (mut left, mut dirty) = (false, self.publishing.target_dirty);
        let s = &mut self.vdata.settings.publish;
        // what is typed is kept apart: an address with a password in it never reaches the settings file, which travels with the vault (SEC-010)
        let typed = self.publishing.url_text.get_or_insert_with(|| s.url.clone());
        anchor(ui, "publishing.url", |ui| {
            ui.label("Address of the publish repository");
            let r = ui.add(egui::TextEdit::singleline(typed).hint_text("https://git.example.org/me/site.git").desired_width(420.0));
            if r.changed() && (typed.trim().is_empty() || crate::gitsync::check_url(typed).is_ok()) {
                s.url = typed.trim().to_string();
                changed = true;
            }
            dirty |= r.changed();
            left |= r.lost_focus() && dirty;
        });
        anchor(ui, "publishing.branch", |ui| {
            ui.horizontal(|ui| {
                ui.label("Branch");
                let r = ui.add(egui::TextEdit::singleline(&mut s.branch).hint_text(publish::DEFAULT_BRANCH).desired_width(160.0));
                changed |= r.changed();
                dirty |= r.changed();
                left |= r.lost_focus() && dirty;
            });
        });
        self.publishing.target_dirty = dirty && !left;
        let shown = PublishSettings { url: self.publishing.url_text.clone().unwrap_or_default(), ..self.vdata.settings.publish.clone() };
        match publish::target(&shown) {
            Err(e) if !shown.url.trim().is_empty() => {
                theme::problem(ui, e);
            }
            _ => {
                theme::hint(ui, "A repository of its own, never the one this vault syncs with. Sign in as for git sync: a token for https:// addresses (Settings → Sync keeps it for the server and user name) or an SSH key; a password in the address is refused.");
            }
        }
        if left {
            self.publishing.target = None;
            self.start_publish_look();
        }
        changed
    }

    /// The public address of the built site (SITE-081): optional, and kept only when it is allowed.
    fn publishing_site_address_ui(&mut self, ui: &mut Ui) -> bool {
        theme::section(ui, "Address of the site");
        let mut changed = false;
        let s = &mut self.vdata.settings.publish;
        let typed = self.publishing.site_address_text.get_or_insert_with(|| s.site_address.clone());
        anchor(ui, "publishing.site_address", |ui| {
            ui.label("Address of the site (optional)");
            let r = ui.add(egui::TextEdit::singleline(typed).hint_text("https://wiki.example.org/").desired_width(420.0));
            if r.changed() {
                if let Ok(kept) = crate::site_export::address::check(typed) {
                    s.site_address = kept.unwrap_or_default();
                    changed = true;
                }
            }
        });
        match crate::site_export::address::check(typed) {
            Err(e) => {
                theme::problem(ui, e);
            }
            Ok(_) => {
                theme::hint(ui, "Where the built site is served. With it the site has a sitemap.xml and a robots.txt for search engines and its links work below a path such as https://host/wiki/; without it neither is written. The site map page for people (/sitemap.html) is always there.");
            }
        }
        changed
    }

    /// Publish, its progress, its result and who published last (SITE-065, SITE-067, SITE-072).
    fn publishing_run_ui(&mut self, ui: &mut Ui) {
        theme::section(ui, "Publish and check");
        // the address that is typed is the one of the settings (one with a password in it is not kept, so Publish waits)
        let typed_is_kept = self.publishing.url_text.as_deref().is_none_or(|t| t.trim() == self.vdata.settings.publish.url);
        let ready = typed_is_kept && publish::target(&self.vdata.settings.publish).is_ok();
        anchor(ui, "publishing.publish", |ui| {
            ui.horizontal(|ui| {
                let b = ui.add_enabled_ui(ready, |ui| busy::ui::button(ui, &self.busy, &[&PUBLISH, &PUBLISH_LOOK], egui::Button::new("Publish"))).inner;
                if b.on_hover_text("Write the site and push it to the address as one snapshot").clicked() {
                    self.start_publish(false);
                }
                if busy::ui::status(ui, &self.busy, &PUBLISH) {
                    let step = self.publishing.step.lock().map(|s| s.clone()).unwrap_or_default();
                    theme::hint(ui, step);
                }
            });
        });
        if !self.busy.is_running(&PUBLISH) {
            self.publishing_result_ui(ui);
        }
        anchor(ui, "publishing.last", |ui| self.publishing_last_ui(ui, ready));
    }

    /// How the last publish of this session ended: the summary and every warning, or the reason.
    fn publishing_result_ui(&mut self, ui: &mut Ui) {
        let Some((when, result)) = self.publishing.last.clone() else { return };
        match result {
            Ok(summary) => {
                theme::hint(ui, format!("Published at {when}: {}", summary.line().trim_start_matches("Published ")));
                if let Some(why) = &summary.empty_because {
                    theme::hint(ui, format!("{why}."));
                }
                if !summary.warnings.is_empty() {
                    theme::hint(ui, format!("{}:", plural(summary.warnings.len(), "warning", "warnings")));
                    for w in &summary.warnings {
                        theme::hint(ui, format!("• {w}"));
                    }
                }
            }
            Err(e) => {
                let problem = self.publish_problem(&e);
                theme::problem(ui, format!("Publishing at {when} did not work: {}", problem.as_ref().map_or(e.clone(), |p| p.words())));
                if let Some(heading) = problem.and_then(|p| p.guide()) {
                    if ui.link("How to fix this").clicked() {
                        self.open_user_guide_at(test_sync::GUIDE_NOTE, heading);
                    }
                }
            }
        }
    }

    /// From which device and when the target was published last (SITE-072), read from its last commit.
    fn publishing_last_ui(&mut self, ui: &mut Ui, ready: bool) {
        if ready && !self.publishing.looked_once && self.persist_config && !self.busy.is_running(&PUBLISH) {
            self.start_publish_look();
        }
        ui.horizontal(|ui| {
            let b = ui.add_enabled_ui(ready, |ui| busy::ui::button(ui, &self.busy, &[&PUBLISH, &PUBLISH_LOOK], egui::Button::new("Check the target").small())).inner;
            if b.on_hover_text("Ask the server who published the branch last, and when").clicked() {
                self.publishing.target = None;
                self.start_publish_look();
            }
            if busy::ui::status(ui, &self.busy, &PUBLISH_LOOK) {
                return;
            }
            let shown = self.publishing.target.clone().filter(|(key, _)| publish::target(&self.vdata.settings.publish).ok().as_ref() == Some(key));
            match shown.map(|(_, answer)| answer) {
                None => {
                    theme::hint(ui, "Not looked at yet.");
                }
                Some(Ok(None)) => {
                    theme::hint(ui, "Nothing is published at the target yet.");
                }
                Some(Ok(Some(last))) => {
                    let m = &last.meta;
                    let who = match (&m.device, m.time) {
                        (Some(d), Some(t)) => format!("from {d} at {}", local(t)),
                        (Some(d), None) => format!("from {d}"),
                        (None, Some(t)) => format!("at {}", local(t)),
                        (None, None) => "by a program that is not Shale".to_string(),
                    };
                    let vault = match (&m.vault_name, last.this_vault) {
                        (Some(n), false) => format!(", by the vault “{n}”, not this one"),
                        (None, false) if m.vault_id.is_some() => ", by another vault".to_string(),
                        _ => String::new(),
                    };
                    theme::hint(ui, format!("Last published {who}{vault}."));
                }
                Some(Err(e)) => {
                    let problem = self.publish_problem(&e);
                    theme::problem(ui, format!("Could not look at the target: {}", problem.as_ref().map_or(e.clone(), |p| p.words())));
                    if let Some(heading) = problem.and_then(|p| p.guide()) {
                        if ui.link("How to fix this").clicked() {
                            self.open_user_guide_at(test_sync::GUIDE_NOTE, heading);
                        }
                    }
                }
            }
        });
    }
}
