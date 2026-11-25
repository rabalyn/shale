//! The compose window and the lists of drafts and outbox (ADR-0010, MAIL-126 to MAIL-136, MAIL-150,
//! MAIL-151): what is typed, how it is drawn, and the questions that are asked before a mail goes out
//! or a window is closed. The state changes that touch disk and the network are in `mail/ui.rs`.
//!
//! The compose view replaces the three panes of the mail view while a mail is being written. The body
//! is a plain multi-line text field (a mail is plain text; the note editor belongs to notes and a vault).

use crate::app::muted;
use crate::clickable::ComboBoxPointer;
use crate::config::Config;
use crate::keys::Action as KeyAction;
use crate::mail::compose::{Confirm, Draft};
use crate::mail::outbox::{DraftInfo, Item, State};
use crate::mail::ui::MailView;
use crate::theme;
use eframe::egui::{self, Key, Modifiers, Pos2, Rect, RichText, Ui, UiBuilder, Vec2};
use std::path::PathBuf;
use std::time::Instant;

/// How long after the last keystroke the draft is saved (MAIL-130).
pub const AUTOSAVE_AFTER: std::time::Duration = std::time::Duration::from_millis(1500);

/// What the mail view knows of the open vault, for attachments and for completing addresses.
#[derive(Clone, Debug, Default)]
pub struct VaultInfo {
    pub root: PathBuf,
    pub generation: u64,
    /// Vault paths and whether the file is a note.
    pub files: Vec<(String, bool)>,
    /// Name and e-mail address of the people notes.
    pub people: Vec<(String, String)>,
}

/// The address fields and the other text fields (for focus and completion).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Field {
    To,
    Cc,
    Bcc,
    Subject,
    Body,
}

impl Field {
    pub fn is_address(self) -> bool {
        matches!(self, Field::To | Field::Cc | Field::Bcc)
    }
    pub fn key(self) -> &'static str {
        match self {
            Field::To => "to",
            Field::Cc => "cc",
            Field::Bcc => "bcc",
            Field::Subject => "subject",
            Field::Body => "body",
        }
    }
}

/// A question the compose window waits for an answer to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ask {
    /// Close with changes: keep the draft, or discard it?
    Close,
    /// Send anyway? (the first of these is shown)
    Confirm(Vec<Confirm>),
}

/// Choosing a file or a note of the vault to attach.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picker {
    pub notes_only: bool,
    pub query: String,
}

/// A missing-attachment alert and whether the person hid it (it still stops sending).
pub struct AttAlertState {
    pub alert: crate::mail::outbox::AttAlert,
    pub dismissed: bool,
}

pub struct Compose {
    pub draft: Draft,
    /// Counts every change the person made; 0 = untouched since it was opened.
    pub rev: u64,
    /// The revision that is on disk (None: nothing of this draft is on disk).
    pub saved_rev: Option<u64>,
    /// The account folder the draft files are in now (it changes when the sender does).
    pub saved_account: Option<String>,
    pub last_edit: Option<Instant>,
    pub error: Option<String>,
    pub ask: Option<Ask>,
    /// Attachments that could not be read back from a saved draft, and the like.
    pub notes: Vec<String>,
    /// Attachments a cut-off replacement left missing; sending waits until each is restored or removed (MAIL-216).
    pub att_alerts: Vec<AttAlertState>,
    /// Completion candidates: name and address.
    pub contacts: Vec<(String, String)>,
    pub active: Option<Field>,
    pub suggest_cursor: usize,
    popup: Option<Rect>,
    pub focus: Option<Field>,
    pub path: String,
    pub picker: Option<Picker>,
    /// When the draft was last saved (for "Saved").
    pub saved_at: Option<i64>,
    /// Signing or encrypting is going on in a job (ADR-0011); setting this flag cancels it. Nothing is editable meanwhile.
    pub crypting: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    /// The keys found for the recipients in the local keyrings, and what they were looked up for (MAIL-192).
    pub keys: Vec<crate::mail::crypto::KeyLookup>,
    pub keys_for: Option<String>,
    /// What a lookup is waiting to start for (debounce) and since when; and whether one is running.
    pub keys_wanted: Option<(String, Instant)>,
    pub keys_running: bool,
}

impl Compose {
    pub fn new(draft: Draft) -> Compose {
        Compose {
            draft,
            rev: 0,
            saved_rev: None,
            saved_account: None,
            last_edit: None,
            error: None,
            ask: None,
            notes: vec![],
            att_alerts: vec![],
            contacts: vec![],
            active: None,
            suggest_cursor: 0,
            popup: None,
            focus: None,
            path: String::new(),
            picker: None,
            saved_at: None,
            crypting: None,
            keys: vec![],
            keys_for: None,
            keys_wanted: None,
            keys_running: false,
        }
    }

    /// What the key lookup is about: the standard and the addresses.
    pub fn keys_signature(&self) -> String {
        let addrs: Vec<String> = crate::mail::crypto::recipients_of(&self.draft).into_iter().map(|a| a.0).collect();
        format!("{:?}|{}", self.draft.standard, addrs.join(","))
    }

    /// A draft that was saved before: its files are on disk and match what is shown.
    pub fn from_saved(draft: Draft, now: i64) -> Compose {
        let mut c = Compose::new(draft);
        c.saved_rev = Some(0);
        c.saved_account = Some(c.draft.account.clone());
        c.saved_at = Some(now);
        c
    }

    pub fn touch(&mut self) {
        self.rev += 1;
        self.last_edit = Some(Instant::now());
        self.error = None;
    }

    /// Is there anything in the draft that the person would miss?
    pub fn has_content(&self) -> bool {
        let d = &self.draft;
        !(d.to.trim().is_empty() && d.cc.trim().is_empty() && d.bcc.trim().is_empty() && d.subject.trim().is_empty() && d.attachments.is_empty() && body_is_empty(d))
    }

    /// Changes that are not on disk.
    pub fn unsaved(&self) -> bool {
        self.saved_rev != Some(self.rev) && (self.rev > 0 || self.saved_rev.is_some())
    }

    pub fn add_attachment(&mut self, a: crate::mail::compose::Att) -> Result<(), String> {
        if self.draft.attachment_bytes() + a.data.len() > crate::mail::compose::MAX_ATTACHMENTS_TOTAL {
            return Err(format!("The attachments together may not be bigger than {} MB", crate::mail::compose::MAX_ATTACHMENTS_TOTAL >> 20));
        }
        self.draft.attachments.push(a);
        self.touch();
        Ok(())
    }
}

/// A body of nothing, or of only a signature block or an attribution line, is empty for "is there anything to lose".
fn body_is_empty(d: &Draft) -> bool {
    let before_sig: Vec<&str> = d.body.lines().take_while(|l| l.trim_end() != "--").collect();
    before_sig.iter().all(|l| l.trim().is_empty())
}

/// The part of an address field after the last comma: what is being typed.
pub fn current_token(text: &str) -> &str {
    text.rsplit([',', ';']).next().unwrap_or("").trim_start()
}

/// Replace the token being typed by a chosen address (`Name <a@b>`), and add the comma and space for the next one.
pub fn complete(text: &str, choice: &str) -> String {
    let cut = text.rfind([',', ';']).map(|i| i + 1).unwrap_or(0);
    let head = text[..cut].trim_end();
    if head.is_empty() {
        format!("{choice}, ")
    } else {
        format!("{head} {choice}, ")
    }
}

/// `Name <address>` or the bare address.
pub fn mailbox_text(name: &str, addr: &str) -> String {
    let n = name.trim();
    if n.is_empty() || n.eq_ignore_ascii_case(addr) {
        addr.to_string()
    } else if n.contains([',', '"', '<', '>', '(', ')', ';', ':', '@', '\\']) {
        format!("\"{}\" <{addr}>", n.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        format!("{n} <{addr}>")
    }
}

/// The best candidates for what is typed: names and addresses that contain every word, those that start with it first.
pub fn suggestions(contacts: &[(String, String)], typed: &str, already: &str, limit: usize) -> Vec<(String, String)> {
    let q = typed.trim().to_lowercase();
    if q.chars().count() < 2 {
        return vec![];
    }
    let words: Vec<&str> = q.split_whitespace().collect();
    let have: Vec<String> = crate::mail::compose::split_list(already).iter().filter_map(|p| crate::mail::compose::bare_address(p)).collect();
    let mut scored: Vec<(u8, usize, &(String, String))> = contacts
        .iter()
        .enumerate()
        .filter(|(_, (_, a))| !have.contains(&a.to_lowercase()))
        .filter_map(|(i, c)| {
            let hay = format!("{} {}", c.0.to_lowercase(), c.1.to_lowercase());
            if !words.iter().all(|w| hay.contains(w)) {
                return None;
            }
            let starts = c.0.to_lowercase().starts_with(&q) || c.1.to_lowercase().starts_with(&q);
            Some((if starts { 0 } else { 1 }, i, c))
        })
        .collect();
    scored.sort_by_key(|(s, i, _)| (*s, *i));
    scored.into_iter().take(limit).map(|(_, _, c)| c.clone()).collect()
}

fn rec(rects: &mut Vec<(String, Rect)>, key: impl Into<String>, r: &egui::Response) {
    rects.push((key.into(), r.rect));
}

fn field_id(f: Field) -> egui::Id {
    egui::Id::unique(("mail-compose", f.key()))
}

/// What the compose view asks for.
#[derive(Default)]
pub struct ComposeOut {
    pub send: bool,
    pub save: bool,
    pub close: bool,
    pub keep_and_close: bool,
    pub discard_and_close: bool,
    pub cancel_ask: bool,
    pub confirm_yes: bool,
    pub attach_path: bool,
    pub attach_vault: Option<String>,
    pub remove_att: Option<usize>,
    /// The alert of a missing attachment (by file name): restore the old file, remove the attachment, hide the alert.
    pub att_restore: Option<String>,
    pub att_remove_missing: Option<String>,
    pub att_dismiss: Option<String>,
    pub account: Option<String>,
    /// Stop the signing or encrypting that is going on.
    pub cancel_crypt: bool,
}

/// The Sign / Encrypt toggles, the standard and the key found for every recipient (MAIL-192, MAIL-193). Only the local keyrings
/// are asked; a missing key is said, never fetched.
fn security_row(ui: &mut Ui, c: &mut Compose, acct: Option<&crate::config::MailAccount>, consent: bool, label_w: f32, rects: &mut Vec<(String, Rect)>, touched: &mut bool) {
    use crate::config::CryptoStandard::{OpenPgp, Smime};
    let has = |s| acct.is_some_and(|a| !a.key_for(s).is_empty());
    let standards: Vec<_> = [OpenPgp, Smime].into_iter().filter(|s| has(*s)).collect();
    ui.horizontal_wrapped(|ui| {
        ui.add_sized([label_w, 20.0], egui::Label::new(muted("Security")));
        if standards.is_empty() {
            crate::theme::prose(ui, muted("To sign or encrypt, choose a key in the account settings (Encryption).").size(12.5));
            return;
        }
        if !standards.contains(&c.draft.standard) {
            c.draft.standard = standards[0];
            c.draft.sign = false;
            c.draft.encrypt = false;
        }
        if standards.len() > 1 {
            for s in &standards {
                let r = crate::clickable::pointer(ui.radio(c.draft.standard == *s, s.label()));
                rec(rects, format!("mail:compose:std:{s:?}"), &r);
                if r.clicked() && c.draft.standard != *s {
                    c.draft.standard = *s;
                    *touched = true;
                }
            }
        } else {
            crate::theme::prose(ui, muted(standards[0].label()).size(12.5));
        }
        let r = crate::clickable::pointer(ui.checkbox(&mut c.draft.sign, "Sign"));
        rec(rects, "mail:compose:sign", &r);
        if r.on_hover_text("Sign the mail with your key: the recipient can check it is from you and was not changed").changed() {
            *touched = true;
        }
        let r = crate::clickable::pointer(ui.add_enabled(consent, egui::Checkbox::new(&mut c.draft.encrypt, "Encrypt")));
        rec(rects, "mail:compose:encrypt", &r);
        let r = if consent {
            r.on_hover_text("Encrypt for every recipient (Cc and Bcc too) and for you, so the copy in Sent stays readable")
        } else {
            r.on_disabled_hover_text("Switch encryption on in the settings first (the Sent copy is kept decrypted on this computer)")
        };
        if r.changed() {
            *touched = true;
        }
    });
    if c.draft.encrypt && !standards.is_empty() {
        let now_sig = c.keys_signature();
        let fresh = c.keys_for.as_ref() == Some(&now_sig);
        for (i, (addr, _bcc)) in crate::mail::crypto::recipients_of(&c.draft).into_iter().enumerate() {
            let found = c.keys.iter().find(|k| k.address == addr);
            let (text, color) = match (fresh, found) {
                (_, Some(k)) if k.error.is_some() => (format!("{addr}: {}", k.error.clone().unwrap_or_default()), theme::palette().link_missing),
                (true, Some(k)) => match &k.key {
                    Some(key) => (format!("{addr}: key found ({}; {}; {})", key.label(), key.trust_words(), crate::mail::ui::short_fingerprint(&key.fingerprint)), theme::palette().tag),
                    None => (format!("{addr}: no key: this mail cannot be encrypted for them (import their key)"), theme::palette().link_missing),
                },
                _ => (format!("{addr}: looking for a key…"), theme::palette().muted),
            };
            ui.horizontal(|ui| {
                ui.add_space(label_w + 4.0);
                let r = ui.label(RichText::new(text).size(12.5).color(color));
                rec(rects, format!("mail:compose:key:{i}"), &r);
            });
        }
        ui.horizontal_wrapped(|ui| {
            ui.add_space(label_w + 4.0);
            let mut notes = vec!["Also encrypted for you, so the copy in Sent can be read. The subject and the addresses of an encrypted mail are not encrypted.".to_string()];
            if c.draft.standard == Smime && !c.draft.bcc.trim().is_empty() {
                notes.push("S/MIME shows every recipient's certificate in the message: people can tell who was on Bcc.".into());
            } else if !c.draft.bcc.trim().is_empty() {
                notes.push("Bcc recipients are hidden recipients: the others cannot see their keys.".into());
            }
            crate::theme::hint(ui, notes.join(" "));
        });
    }
}

impl MailView {
    /// Draw the compose view into `body` (replacing the three panes). Returns what was asked for.
    pub fn compose_view(&mut self, ui: &mut Ui, cfg: &Config, body: Rect, rects: &mut Vec<(String, Rect)>) -> ComposeOut {
        let mut out = ComposeOut::default();
        let ctx = ui.ctx().clone();
        // keys first, so a text field does not see Ctrl+Enter or Esc (MAIL-150)
        let (send_key, esc) = ui.input_mut(|i| (i.consume_key(Modifiers::COMMAND, Key::Enter), i.consume_key(Modifiers::NONE, Key::Escape)));
        let crypting = self.compose.as_ref().is_some_and(|c| c.crypting.is_some());
        let asking = self.compose.as_ref().is_some_and(|c| c.ask.is_some()) || crypting;
        if send_key && !asking {
            out.send = true;
        }
        if esc {
            if asking {
                out.cancel_ask = true;
            } else if self.compose.as_ref().is_some_and(|c| c.picker.is_some()) {
                if let Some(c) = self.compose.as_mut() {
                    c.picker = None;
                }
            } else if self.compose.as_ref().is_some_and(|c| c.active.is_some() && c.popup_open()) {
                if let Some(c) = self.compose.as_mut() {
                    c.active = None;
                }
            } else {
                out.close = true;
            }
        }
        let consent = self.consent;
        let acct = self.compose.as_ref().and_then(|c| cfg.mail_accounts.iter().find(|a| a.id == c.draft.account)).cloned();
        let accounts: Vec<(String, String)> = cfg.mail_accounts.iter().map(|a| (a.id.clone(), if a.name.is_empty() { a.host.clone() } else { a.name.clone() })).collect();
        let have_vault = self.vault.is_some();
        let Some(c) = self.compose.as_mut() else { return out };
        ui.painter().line_segment([body.left_top(), body.right_top()], egui::Stroke::new(1.0, theme::palette().border));
        ui.scope_builder(UiBuilder::new().max_rect(body.shrink2(Vec2::new(18.0, 10.0))), |ui| {
            ui.set_clip_rect(body.intersect(ui.clip_rect()));
            let mut touched = false;
            ui.horizontal(|ui| {
                let r = ui.add_enabled(!asking, egui::Button::new(RichText::new("Send").strong()));
                rec(rects, "mail:compose:send", &r);
                if r.on_hover_text("Send (Ctrl+Enter). The mail waits in the outbox for a few seconds, so you can still take it back.").clicked() {
                    out.send = true;
                }
                let r = ui.add_enabled(!asking, egui::Button::new("Save draft"));
                rec(rects, "mail:compose:save", &r);
                if r.on_hover_text("Drafts are also saved by themselves after a pause").clicked() {
                    out.save = true;
                }
                let r = ui.add_enabled(!asking, egui::Button::new("Close"));
                rec(rects, "mail:compose:close", &r);
                if r.on_hover_text("Close (Esc). If there are changes you are asked whether to keep the draft.").clicked() {
                    out.close = true;
                }
                ui.add_space(8.0);
                if crypting {
                    ui.label(RichText::new("Signing / encrypting… (a passphrase or PIN dialog may open)").color(theme::palette().text));
                    let r = ui.button("Cancel");
                    rec(rects, "mail:compose:crypt-cancel", &r);
                    if r.on_hover_text("Stop: the mail stays here, nothing is sent (Esc)").clicked() {
                        out.cancel_crypt = true;
                    }
                }
                let status = if c.unsaved() {
                    "Not saved yet".to_string()
                } else if let Some(t) = c.saved_at {
                    format!("Draft saved {}", crate::mail::ui::full_date(t).split(' ').nth(1).unwrap_or(""))
                } else {
                    String::new()
                };
                crate::theme::hint(ui, status);
            });
            if let Some(e) = &c.error {
                ui.label(RichText::new(e.as_str()).color(theme::palette().link_missing));
            }
            for n in &c.notes {
                ui.label(RichText::new(n.as_str()).size(12.5).color(theme::palette().link_missing));
            }
            // an attachment that a cut-off replacement left missing (MAIL-216, MAIL-217)
            for (i, al) in c.att_alerts.iter().enumerate() {
                if al.dismissed {
                    continue;
                }
                ui.label(RichText::new(al.alert.message()).size(12.5).color(theme::palette().link_missing));
                ui.horizontal(|ui| {
                    if al.alert.trash_present {
                        let r = ui.small_button("Restore the old file");
                        rec(rects, format!("mail:compose:att-restore:{i}"), &r);
                        if r.clicked() {
                            out.att_restore = Some(al.alert.file.clone());
                        }
                    }
                    let r = ui.small_button("Remove the attachment");
                    rec(rects, format!("mail:compose:att-alert-remove:{i}"), &r);
                    if r.clicked() {
                        out.att_remove_missing = Some(al.alert.file.clone());
                    }
                    let r = ui.small_button("Dismiss");
                    rec(rects, format!("mail:compose:att-dismiss:{i}"), &r);
                    if r.clicked() {
                        out.att_dismiss = Some(al.alert.file.clone());
                    }
                });
            }
            ui.add_space(4.0);
            // nothing is editable while GnuPG works: what is signed is what was typed
            ui.add_enabled_ui(!crypting, |ui| {
                egui::ScrollArea::vertical().id_salt("mail-compose-scroll").auto_shrink([false, false]).show(ui, |ui| {
                    let label_w = 64.0;
                    // From
                    ui.horizontal(|ui| {
                        ui.add_sized([label_w, 20.0], egui::Label::new(muted("From")));
                        let from = format!(
                            "{}{}",
                            if c.draft.from_name.is_empty() { String::new() } else { format!("{} ", c.draft.from_name) },
                            if c.draft.from_addr.is_empty() { "(no address: see the account settings)".to_string() } else { format!("<{}>", c.draft.from_addr) }
                        );
                        if accounts.len() > 1 {
                            let cur = accounts.iter().find(|a| a.0 == c.draft.account).map(|a| a.1.clone()).unwrap_or_default();
                            let r = egui::ComboBox::from_id_salt("mail-compose-account").selected_text(format!("{cur}  ·  {from}")).show_pointer(ui, |ui| {
                                for (id, name) in &accounts {
                                    let r = ui.selectable_label(&c.draft.account == id, name.clone());
                                    rec(rects, format!("mail:compose:account:{id}"), &r);
                                    if r.clicked() {
                                        out.account = Some(id.clone());
                                    }
                                }
                            });
                            rec(rects, "mail:compose:from", &r.response);
                        } else {
                            ui.label(RichText::new(from).color(theme::palette().text));
                        }
                    });
                    for f in [Field::To, Field::Cc, Field::Bcc] {
                        ui.horizontal(|ui| {
                            ui.add_sized(
                                [label_w, 20.0],
                                egui::Label::new(muted(match f {
                                    Field::To => "To",
                                    Field::Cc => "Cc",
                                    _ => "Bcc",
                                })),
                            );
                            let id = field_id(f);
                            let text = match f {
                                Field::To => &mut c.draft.to,
                                Field::Cc => &mut c.draft.cc,
                                _ => &mut c.draft.bcc,
                            };
                            let r =
                                ui.add(egui::TextEdit::singleline(text).id(id).desired_width(f32::INFINITY).hint_text(if f == Field::Bcc { "hidden recipients (never shown in the mail)" } else { "names or addresses, separated by commas" }));
                            rec(rects, format!("mail:compose:{}", f.key()), &r);
                            if c.focus == Some(f) {
                                r.request_focus();
                                c.focus = None;
                            }
                            if r.changed() {
                                touched = true;
                            }
                            if r.has_focus() {
                                c.active = Some(f);
                            }
                        });
                    }
                    ui.horizontal(|ui| {
                        ui.add_sized([label_w, 20.0], egui::Label::new(muted("Subject")));
                        let r = ui.add(egui::TextEdit::singleline(&mut c.draft.subject).id(field_id(Field::Subject)).desired_width(f32::INFINITY));
                        rec(rects, "mail:compose:subject", &r);
                        if c.focus == Some(Field::Subject) {
                            r.request_focus();
                            c.focus = None;
                        }
                        if r.changed() {
                            touched = true;
                        }
                        if r.has_focus() {
                            c.active = None;
                        }
                    });
                    security_row(ui, c, acct.as_ref(), consent, label_w, rects, &mut touched);
                    // attachments
                    if !c.draft.attachments.is_empty() {
                        for (i, a) in c.draft.attachments.iter().enumerate() {
                            ui.horizontal(|ui| {
                                ui.add_space(label_w + 4.0);
                                ui.label(RichText::new(format!("📎 {}", a.name)).color(theme::palette().text));
                                crate::theme::hint(ui, crate::mail::ui::size_text(a.data.len() as u64));
                                let r = ui.small_button("Remove");
                                rec(rects, format!("mail:compose:att-remove:{i}"), &r);
                                if r.clicked() {
                                    out.remove_att = Some(i);
                                }
                            });
                        }
                    }
                    ui.horizontal_wrapped(|ui| {
                        ui.add_space(label_w + 4.0);
                        crate::theme::hint(ui, "Attach a file:");
                        let r = ui.add(egui::TextEdit::singleline(&mut c.path).desired_width(260.0).hint_text("path of a file on this computer"));
                        rec(rects, "mail:compose:att-path", &r);
                        let go = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                        let b = ui.small_button("Attach");
                        rec(rects, "mail:compose:att-add", &b);
                        if (b.clicked() || go) && !c.path.trim().is_empty() {
                            out.attach_path = true;
                        }
                        let b = ui.add_enabled(have_vault, egui::Button::new("From vault…").small());
                        rec(rects, "mail:compose:att-vault", &b);
                        if b.on_hover_text("A file of the open vault, also from the media folder").clicked() {
                            c.picker = Some(Picker { notes_only: false, query: String::new() });
                        }
                        let b = ui.add_enabled(have_vault, egui::Button::new("Attach note…").small());
                        rec(rects, "mail:compose:att-note", &b);
                        if b.on_hover_text("The markdown text of a note, as a .md file").clicked() {
                            c.picker = Some(Picker { notes_only: true, query: String::new() });
                        }
                    });
                    ui.add_space(4.0);
                    let h = ui.available_height().max(240.0);
                    let r = ui.add_sized([ui.available_width(), h], egui::TextEdit::multiline(&mut c.draft.body).id(field_id(Field::Body)).desired_width(f32::INFINITY).lock_focus(true));
                    rec(rects, "mail:compose:body", &r);
                    if c.focus == Some(Field::Body) {
                        r.request_focus();
                        // the cursor goes to the top: a new mail starts above its signature, an answer is typed above or below the quote
                        if let Some(mut st) = egui::text_edit::TextEditState::load(ui.ctx(), field_id(Field::Body)) {
                            let end = if c.draft.kind == crate::mail::compose::Kind::New { 0 } else { c.draft.body.find("\n-- \n").map(|i| c.draft.body[..i].chars().count()).unwrap_or_else(|| c.draft.body.chars().count()) };
                            st.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(end))));
                            st.store(ui.ctx(), field_id(Field::Body));
                        }
                        c.focus = None;
                    }
                    if r.changed() {
                        touched = true;
                    }
                    if r.has_focus() {
                        c.active = None;
                    }
                })
            });
            if touched {
                c.touch();
            }
        });
        // completion of the address being typed
        self.suggest_popup(&ctx, rects);
        self.compose_dialogs(&ctx, rects, &mut out);
        out
    }

    fn suggest_popup(&mut self, ctx: &egui::Context, rects: &mut Vec<(String, Rect)>) {
        let Some(c) = self.compose.as_mut() else { return };
        c.popup = None;
        if c.ask.is_some() || c.picker.is_some() {
            return;
        }
        let Some(f) = c.active.filter(|f| f.is_address()) else { return };
        let text = match f {
            Field::To => c.draft.to.clone(),
            Field::Cc => c.draft.cc.clone(),
            _ => c.draft.bcc.clone(),
        };
        let list = suggestions(&c.contacts, current_token(&text), &text, 6);
        if list.is_empty() {
            return;
        }
        c.suggest_cursor = c.suggest_cursor.min(list.len() - 1);
        // ↑ ↓ choose, Tab or Enter takes (the keys are taken before the field sees them next frame)
        let (up, down, take) = ctx.input_mut(|i| (i.consume_key(Modifiers::NONE, Key::ArrowUp), i.consume_key(Modifiers::NONE, Key::ArrowDown), i.consume_key(Modifiers::NONE, Key::Tab)));
        if up {
            c.suggest_cursor = c.suggest_cursor.saturating_sub(1);
        }
        if down {
            c.suggest_cursor = (c.suggest_cursor + 1).min(list.len() - 1);
        }
        let mut chosen: Option<usize> = take.then_some(c.suggest_cursor);
        let Some(rect) = rects.iter().rev().find(|r| r.0 == format!("mail:compose:{}", f.key())).map(|r| r.1) else { return };
        let area = egui::Area::new(egui::Id::unique("mail-suggest")).order(egui::Order::Foreground).fixed_pos(Pos2::new(rect.min.x, rect.max.y + 2.0)).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_min_width(rect.width().min(420.0));
                for (i, (n, a)) in list.iter().enumerate() {
                    let label = if n.is_empty() { a.clone() } else { format!("{n}  <{a}>") };
                    let r = ui.selectable_label(i == c.suggest_cursor, label);
                    rects.push((format!("mail:suggest:{i}"), r.rect));
                    if r.clicked() {
                        chosen = Some(i);
                    }
                }
            });
        });
        c.popup = Some(area.response.rect);
        if let Some(i) = chosen {
            let (n, a) = &list[i];
            let new = complete(&text, &mailbox_text(n, a));
            match f {
                Field::To => c.draft.to = new,
                Field::Cc => c.draft.cc = new,
                _ => c.draft.bcc = new,
            }
            c.touch();
            c.suggest_cursor = 0;
            c.focus = Some(f);
            // the cursor goes to the end of the field
            let id = field_id(f);
            if let Some(mut st) = egui::text_edit::TextEditState::load(ctx, id) {
                let n = match f {
                    Field::To => c.draft.to.chars().count(),
                    Field::Cc => c.draft.cc.chars().count(),
                    _ => c.draft.bcc.chars().count(),
                };
                st.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(n))));
                st.store(ctx, id);
            }
        }
    }

    /// The questions: keep or discard on close, send anyway, and the picker for vault files and notes.
    fn compose_dialogs(&mut self, ctx: &egui::Context, rects: &mut Vec<(String, Rect)>, out: &mut ComposeOut) {
        let vault = self.vault.clone();
        let Some(c) = self.compose.as_mut() else { return };
        if let Some(ask) = c.ask.clone() {
            let title = match &ask {
                Ask::Close => "Keep this draft?",
                Ask::Confirm(_) => "Send this mail?",
            };
            let resp = crate::dialog::Dialog::new("compose-ask", title).width(420.0).non_modal().show(ctx, |ui| match &ask {
                Ask::Close => {
                    crate::theme::prose(ui, muted("You have written something that is not sent. A kept draft is saved and can be opened from Drafts; a discarded one is moved to the mail trash folder (nothing is deleted)."));
                    ui.horizontal(|ui| {
                        let r = ui.button(RichText::new("Keep draft").strong());
                        rec(rects, "mail:compose:keep", &r);
                        if r.clicked() {
                            out.keep_and_close = true;
                        }
                        let r = ui.button("Discard");
                        rec(rects, "mail:compose:discard", &r);
                        if r.clicked() {
                            out.discard_and_close = true;
                        }
                        let r = ui.button("Keep writing");
                        rec(rects, "mail:compose:keep-writing", &r);
                        if r.clicked() {
                            out.cancel_ask = true;
                        }
                    });
                }
                Ask::Confirm(list) => {
                    let q = list.first().map(|q| q.question()).unwrap_or("");
                    ui.label(q);
                    ui.horizontal(|ui| {
                        let r = ui.button("Send anyway");
                        rec(rects, "mail:compose:confirm-yes", &r);
                        if r.clicked() {
                            out.confirm_yes = true;
                        }
                        let r = ui.button("Back to the mail");
                        rec(rects, "mail:compose:confirm-no", &r);
                        if r.clicked() {
                            out.cancel_ask = true;
                        }
                    });
                }
            });
            // the ✕ in the title bar: back to the mail, as "Keep writing" / "Back to the mail"
            if resp.should_close() {
                out.cancel_ask = true;
            }
        }
        if let (Some(p), Some(v)) = (c.picker.as_mut(), vault) {
            let mut close = false;
            let title = if p.notes_only { "Attach a note" } else { "Attach a file from the vault" };
            let resp = crate::dialog::Dialog::new("compose-picker", title).width(420.0).non_modal().show(ctx, |ui| {
                let r = ui.add(egui::TextEdit::singleline(&mut p.query).desired_width(f32::INFINITY).hint_text("type to filter"));
                rec(rects, "mail:picker:query", &r);
                r.request_focus();
                let q = p.query.to_lowercase();
                let hits: Vec<&(String, bool)> = v.files.iter().filter(|(f, note)| (!p.notes_only || *note) && q.split_whitespace().all(|w| f.to_lowercase().contains(w))).take(14).collect();
                for (i, (f, _)) in hits.iter().enumerate() {
                    let b = ui.selectable_label(false, f.as_str());
                    rec(rects, format!("mail:picker:{i}"), &b);
                    if b.clicked() {
                        out.attach_vault = Some(f.clone());
                    }
                }
                if hits.is_empty() {
                    ui.label(muted("Nothing matches."));
                }
                if ui.button("Cancel").clicked() {
                    close = true;
                }
            });
            close |= resp.should_close();
            if close {
                c.picker = None;
            }
        }
    }
}

impl Compose {
    fn popup_open(&self) -> bool {
        self.popup.is_some()
    }
}

// ---------------------------------------------------------------- drafts and outbox lists

/// Which local list is open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LocalView {
    Drafts(String),
    Outbox(String),
}

/// What the lists of drafts and outbox ask for.
#[derive(Default)]
pub struct LocalOut {
    pub open_draft: Option<(String, String)>,
    pub select: Option<(String, String)>,
    pub undo: Option<(String, String)>,
    pub retry: Option<(String, String)>,
    pub send_again: Option<(String, String)>,
    pub edit: Option<(String, String)>,
    pub discard: Option<(String, String)>,
    pub discard_draft: Option<(String, String)>,
}

/// "in 7 s" for a mail that waits.
pub fn wait_text(send_after: i64, now: i64) -> String {
    let left = send_after - now;
    if left <= 0 {
        "sending now".to_string()
    } else {
        format!("sent in {left} s")
    }
}

impl MailView {
    /// The row that Tab moved to is framed like the pane that has the keyboard (MAIL-073)
    /// and becomes the cursor, so the mail keys act on it; the mail keys know it as the row that
    /// has the focus (`Kbd::row_focus`).
    pub(crate) fn tab_focus(&mut self, ui: &Ui, r: &egui::Response, key: &str, i: usize, rects: &mut Vec<(String, Rect)>) {
        if r.has_focus() {
            ui.painter().rect_stroke(r.rect.shrink(1.0), 0, egui::Stroke::new(2.0, theme::palette().accent), egui::StrokeKind::Inside);
            rects.push((format!("{key}:{i}"), r.rect));
            self.kbd.row_focus = Some(r.id);
            if r.gained_focus() {
                self.cursor = Some(i);
            }
        }
    }

    /// The middle pane when Drafts or Outbox is open.
    pub fn local_list(&mut self, ui: &mut Ui, now: i64, rects: &mut Vec<(String, Rect)>, out: &mut LocalOut) {
        let Some(view) = self.local_view.clone() else { return };
        match view {
            LocalView::Drafts(account) => {
                ui.horizontal(|ui| {
                    ui.add_space(10.0);
                    ui.label(RichText::new(format!("Drafts  ·  {}", self.account_title(&account))).size(13.0).color(theme::palette().muted));
                });
                let list: Vec<DraftInfo> = self.local_drafts.iter().filter(|d| d.account == account).cloned().collect();
                if list.is_empty() {
                    ui.horizontal(|ui| {
                        ui.add_space(10.0);
                        ui.label(muted("No drafts."));
                    });
                }
                for (i, d) in list.iter().enumerate() {
                    let r = ui.add(
                        egui::Button::selectable(
                            false,
                            RichText::new(format!(
                                "{}\n{}  ·  saved {}",
                                if d.subject.trim().is_empty() { "(no subject)" } else { d.subject.as_str() },
                                if d.to.trim().is_empty() { "(no recipient)" } else { d.to.as_str() },
                                crate::mail::ui::full_date(d.saved)
                            ))
                            .size(13.0),
                        )
                        .min_size(Vec2::new(ui.available_width(), 44.0)),
                    );
                    rec(rects, format!("mail:draft:{i}"), &r);
                    self.tab_focus(ui, &r, "mail:draft-focus", i, rects);
                    if self.cursor == Some(i) {
                        ui.painter().rect_filled(r.rect, 0, theme::palette().accent.gamma_multiply(0.25));
                    }
                    if r.clicked() {
                        out.open_draft = Some((d.account.clone(), d.id.clone()));
                    }
                    if r.secondary_clicked() {
                        self.open_menu_at(crate::mail::keys::MenuTarget::Draft(i), ui.input(|i| i.pointer.interact_pos()).unwrap_or(r.rect.center()));
                    }
                }
            }
            LocalView::Outbox(account) => {
                ui.horizontal(|ui| {
                    ui.add_space(10.0);
                    ui.label(RichText::new(format!("Outbox  ·  {}", self.account_title(&account))).size(13.0).color(theme::palette().muted));
                });
                let list: Vec<Item> = self.local_items.iter().filter(|i| i.account == account).cloned().collect();
                if list.is_empty() {
                    ui.horizontal(|ui| {
                        ui.add_space(10.0);
                        ui.label(muted("Nothing waits to be sent."));
                    });
                }
                for (i, it) in list.iter().enumerate() {
                    let selected = self.outbox_sel.as_ref() == Some(&(it.account.clone(), it.id.clone()));
                    let state = match it.state {
                        State::Queued => wait_text(it.send_after, now),
                        s => s.label().to_string(),
                    };
                    let color = match it.state {
                        State::Failed | State::Unsure => theme::palette().link_missing,
                        _ => theme::palette().text,
                    };
                    let r = ui.add(
                        egui::Button::selectable(selected, RichText::new(format!("{}\n{}  ·  {state}", if it.subject.trim().is_empty() { "(no subject)" } else { it.subject.as_str() }, it.recipients.join(", "))).size(13.0).color(color))
                            .min_size(Vec2::new(ui.available_width(), 44.0)),
                    );
                    rec(rects, format!("mail:outbox:{i}"), &r);
                    self.tab_focus(ui, &r, "mail:outbox-focus", i, rects);
                    if self.cursor == Some(i) {
                        ui.painter().rect_filled(r.rect, 0, theme::palette().accent.gamma_multiply(0.25));
                    }
                    if r.clicked() {
                        out.select = Some((it.account.clone(), it.id.clone()));
                    }
                }
            }
        }
    }

    /// The reading pane when an outbox entry is selected: what happened and what can be done.
    pub fn outbox_detail(&mut self, ui: &mut Ui, now: i64, rects: &mut Vec<(String, Rect)>, out: &mut LocalOut) {
        let Some(sel) = self.outbox_sel.clone() else {
            ui.add_space(30.0);
            crate::theme::prose(ui, muted("Select a mail of the outbox to see what happened to it."));
            return;
        };
        let Some(it) = self.local_items.iter().find(|i| (i.account.clone(), i.id.clone()) == sel).cloned() else {
            ui.add_space(30.0);
            ui.label(muted("That mail is not in the outbox any more."));
            return;
        };
        egui::ScrollArea::vertical().id_salt("mail-outbox-detail").auto_shrink([false, false]).show(ui, |ui| {
            let key = (it.account.clone(), it.id.clone());
            ui.label(RichText::new(if it.subject.trim().is_empty() { "(no subject)" } else { it.subject.as_str() }).size(19.0).color(theme::palette().text_strong));
            crate::theme::prose(ui, muted(format!("From {}  ·  to {}", it.from, it.recipients.join(", "))));
            ui.add_space(6.0);
            match it.state {
                State::Queued => {
                    ui.label(RichText::new(format!("Waiting: {}.", wait_text(it.send_after, now))).color(theme::palette().text));
                    let r = self.key_button(ui, "Undo send", KeyAction::MailUndoSend);
                    rec(rects, "mail:outbox:undo", &r);
                    if r.on_hover_text(self.tip("Take the mail back into the editor", KeyAction::MailUndoSend)).clicked() {
                        out.undo = Some(key.clone());
                    }
                }
                State::Sending => {
                    ui.label(RichText::new("Sending…").color(theme::palette().text));
                }
                State::Sent => {
                    ui.label(RichText::new("Sent.").color(theme::palette().text));
                    let r = self.key_button(ui, "Dismiss", KeyAction::MailDelete);
                    rec(rects, "mail:outbox:discard", &r);
                    if r.on_hover_text(self.tip("Move this record to the mail trash folder; the mail itself is in Sent", KeyAction::MailDelete)).clicked() {
                        out.discard = Some(key.clone());
                    }
                }
                State::Failed => {
                    ui.label(RichText::new("This mail was not sent.").size(15.0).color(theme::palette().link_missing));
                    crate::theme::prose(ui, muted(it.error.clone().unwrap_or_default()));
                    ui.horizontal_wrapped(|ui| {
                        for (k, label, slot, action, tip) in [
                            ("mail:outbox:retry", "Retry", 0, KeyAction::MailRetry, self.tip("Send the mail again", KeyAction::MailRetry)),
                            ("mail:outbox:edit", "Edit", 1, KeyAction::MailEdit, self.tip("Take the mail back into the editor", KeyAction::MailEdit)),
                            ("mail:outbox:discard", "Discard", 2, KeyAction::MailDelete, self.tip("Remove this mail from the outbox", KeyAction::MailDelete)),
                        ] {
                            let r = self.key_button(ui, label, action);
                            rec(rects, k, &r);
                            if r.on_hover_text(tip).clicked() {
                                match slot {
                                    0 => out.retry = Some(key.clone()),
                                    1 => out.edit = Some(key.clone()),
                                    _ => out.discard = Some(key.clone()),
                                }
                            }
                        }
                    });
                }
                State::Unsure => {
                    ui.label(RichText::new("This mail may have been sent.").size(15.0).color(theme::palette().link_missing));
                    ui.label(RichText::new("Shale does not know whether the server received it: the connection ended before the server answered. Check your Sent folder (on the server or in another program). If it is there, discard this entry; if it is not, send it again.").color(theme::palette().text));
                    if let Some(e) = &it.error {
                        crate::theme::hint(ui, e.clone());
                    }
                    ui.horizontal_wrapped(|ui| {
                        for (k, label, slot, action, tip) in [
                            ("mail:outbox:send-again", "Send again", 0, KeyAction::MailRetry, self.tip("Send the mail again, after you looked in Sent", KeyAction::MailRetry)),
                            ("mail:outbox:edit", "Edit", 1, KeyAction::MailEdit, self.tip("Take the mail back into the editor", KeyAction::MailEdit)),
                            ("mail:outbox:discard", "Discard", 2, KeyAction::MailDelete, self.tip("Remove this mail from the outbox", KeyAction::MailDelete)),
                        ] {
                            let r = self.key_button(ui, label, action);
                            rec(rects, k, &r);
                            if r.on_hover_text(tip).clicked() {
                                match slot {
                                    0 => out.send_again = Some(key.clone()),
                                    1 => out.edit = Some(key.clone()),
                                    _ => out.discard = Some(key.clone()),
                                }
                            }
                        }
                    });
                }
                State::Undone => {
                    ui.label(muted("Taken back."));
                }
            }
            ui.add_space(6.0);
            ui.separator();
            crate::theme::prose(ui, muted(it.draft.body.clone()).size(13.0));
        });
    }

    fn account_title(&self, id: &str) -> String {
        self.account_names.get(id).cloned().unwrap_or_else(|| id.to_string())
    }
}

/// A strip above the panes while a mail waits in the outbox: the countdown and Undo (MAIL-134).
pub fn queued_text(items: &[Item], now: i64) -> Option<(String, String, String)> {
    items
        .iter()
        .filter(|i| i.state == State::Queued && i.send_after > now)
        .max_by_key(|i| i.created)
        .map(|i| (i.account.clone(), i.id.clone(), format!("“{}” will be sent {}.", if i.subject.trim().is_empty() { "(no subject)" } else { i.subject.as_str() }, wait_text(i.send_after, now).replace("sent ", ""))))
}

/// A colour for the unsure and failed entries of the folder list.
pub fn attention(items: &[Item], account: &str) -> bool {
    items.iter().any(|i| i.account == account && matches!(i.state, State::Failed | State::Unsure))
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec: MAIL-128
    #[test]
    fn completion_replaces_the_word_being_typed_and_skips_what_is_already_there() {
        let contacts = vec![("Bob Builder".to_string(), "bob@x.de".to_string()), ("Carol".to_string(), "carol@y.de".to_string()), ("".to_string(), "bobby@z.de".to_string()), ("Müller, Hans".to_string(), "h@x.de".to_string())];
        assert_eq!(current_token("a@b.de, bo"), "bo");
        assert_eq!(current_token("bo"), "bo");
        assert_eq!(current_token("a@b.de, "), "");
        let s = suggestions(&contacts, "bo", "", 6);
        assert_eq!(s.iter().map(|c| c.1.as_str()).collect::<Vec<_>>(), vec!["bob@x.de", "bobby@z.de"]);
        assert_eq!(suggestions(&contacts, "b", "", 6), vec![], "one letter is too little");
        assert_eq!(suggestions(&contacts, "x.de", "", 6).len(), 2, "by address too");
        assert_eq!(suggestions(&contacts, "builder bob", "", 6).len(), 1, "every word, any order");
        assert_eq!(suggestions(&contacts, "bo", "Bob Builder <bob@x.de>, bo", 6).iter().map(|c| c.1.as_str()).collect::<Vec<_>>(), vec!["bobby@z.de"], "an address already in the field is not offered again");
        assert_eq!(complete("a@b.de, bo", "Bob Builder <bob@x.de>"), "a@b.de, Bob Builder <bob@x.de>, ");
        assert_eq!(complete("bo", "Bob <bob@x.de>"), "Bob <bob@x.de>, ");
        assert_eq!(mailbox_text("Müller, Hans", "h@x.de"), "\"Müller, Hans\" <h@x.de>");
        assert_eq!(mailbox_text("", "h@x.de"), "h@x.de");
        assert_eq!(mailbox_text("Ann", "a@x.de"), "Ann <a@x.de>");
    }

    // spec: MAIL-131
    #[test]
    fn a_compose_window_knows_what_is_unsaved_and_whether_there_is_anything_to_lose() {
        let mut c = Compose::new(crate::mail::compose::new_draft("a", "Ada", "ada@example.com", "Ada"));
        assert!(!c.unsaved() && !c.has_content(), "a new mail with only a signature is untouched");
        c.draft.body = "Hello".into();
        c.touch();
        assert!(c.unsaved() && c.has_content());
        c.saved_rev = Some(c.rev);
        assert!(!c.unsaved());
        c.touch();
        assert!(c.unsaved());
        let mut loaded = Compose::from_saved(c.draft.clone(), 5);
        assert!(!loaded.unsaved());
        loaded.touch();
        assert!(loaded.unsaved());
    }
}
