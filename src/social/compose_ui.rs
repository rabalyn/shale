//! The compose box (SOC-051): a window over a Feeds tab to write a post, a reply or a quote. Send
//! (Ctrl+Enter) never sends at once: "Post this?" with the account and who may see it comes
//! first (SOC-055). The state is in `compose.rs`, the job is started by `SocialState::send_compose`.

use super::actions::Visibility;
use super::compose::{Compose, Stage};
use super::compose_extras_ui as extras;
use super::compose_pictures as pictures;
use super::feed_ui::{inset, short, TabCtx};
use super::state::SocialState;
use super::{Network, SocialAccountEntry};
use crate::clickable::ComboBoxPointer;
use crate::theme;
use eframe::egui::{self, Key, Modifiers, Rect, RichText, Ui, Vec2};

/// The width of the box.
const WIDTH: f32 = 480.0;

/// What the box is told about its surroundings.
#[derive(Clone, Copy)]
struct Env {
    /// the file dialog is open
    picking: bool,
    /// the calendar's week starts on Monday (the date picker)
    monday: bool,
}

/// What the box asked for this frame.
#[derive(Default)]
struct Out {
    /// leave it (Cancel in the first step, Esc)
    close: bool,
    /// Send was pressed: go to the question
    ask: bool,
    /// the question was answered with Post
    post: bool,
    /// the question was answered with Cancel: back to the text
    back: bool,
    add_picture: bool,
    /// "Choose pictures…" was pressed
    pick: bool,
    /// "Scheduled posts…" was pressed
    scheduled_list: bool,
}

fn muted(t: impl Into<String>) -> RichText {
    RichText::new(t).size(crate::fonts::px(12.0)).color(theme::palette().muted)
}

/// Show the box when this tab has one. `vault` is where a relative picture path starts.
pub fn show(ctx: &egui::Context, tab_rect: Rect, st: &mut SocialState, entry: &SocialAccountEntry, tab: &TabCtx) {
    super::schedule::show_list(ctx, tab_rect, st, entry, tab);
    if !st.compose.as_ref().is_some_and(|c| c.tab == tab.id && c.account == entry.account.id) {
        return;
    }
    let info = st.info_for(&entry.account);
    let Some(mut c) = st.compose.take() else { return };
    let network = entry.account.network;
    let mut out = Out::default();
    // keys first, so the text field does not take Ctrl+Enter as a new line
    if tab.keys || c.stage != Stage::Sending {
        ctx.input_mut(|i| {
            if c.stage == Stage::Edit && i.consume_key(Modifiers::COMMAND, Key::Enter) {
                out.ask = true;
            }
            if c.stage != Stage::Sending && i.consume_key(Modifiers::NONE, Key::Escape) {
                if c.stage == Stage::Confirm {
                    out.back = true;
                } else {
                    out.close = true;
                }
            }
        });
    }
    let title = match (&c.reply, &c.quote) {
        (Some(r), _) => format!("Reply to {}", r.author.name()),
        (_, Some(q)) => format!("Quote {}", q.author.name()),
        _ => "New post".to_string(),
    };
    let width = (tab_rect.width() - 40.0).clamp(280.0, WIDTH);
    let mut rects: Vec<(String, Rect)> = vec![];
    let shown =
        egui::Window::new(title).id(egui::Id::unique(("social-compose", tab.id))).collapsible(false).resizable(false).pivot(egui::Align2::CENTER_CENTER).fixed_pos(tab_rect.center()).fixed_size(Vec2::new(width, 0.0)).show(ctx, |ui| {
            ui.set_width(width);
            ui.label(muted(format!("Posting as @{} on {}", entry.account.handle, network.label())));
            ui.add_space(4.0);
            match c.stage {
                Stage::Confirm => confirm(ui, &c, entry, &info, &mut rects, &mut out),
                _ => edit(ui, &mut c, entry, &info, Env { picking: st.intake.picking, monday: st.week_monday }, &mut rects, &mut out),
            }
        });
    st.rects.extend(rects);
    pictures_in(ctx, st, &mut c, &info, shown.map(|r| r.response.rect));
    if out.pick {
        st.pick_pictures(ctx, info.max_pictures.saturating_sub(c.pictures.len()).max(1));
    }
    if out.scheduled_list {
        st.open_scheduled(ctx, entry, tab.id);
    }
    if out.add_picture {
        match c.add_picture(tab.vault.as_deref(), &info) {
            Ok(()) => c.error = None,
            Err(e) => c.error = Some(e),
        }
    }
    if out.ask && c.problem(network, &info).is_none() {
        c.stage = Stage::Confirm;
        c.error = None;
    }
    if out.back {
        c.stage = Stage::Edit;
    }
    if out.close {
        c.forget_pasted();
    } else {
        st.compose = Some(c);
    }
    if out.post {
        st.send_compose(ctx, entry);
    }
}

fn edit(ui: &mut Ui, c: &mut Compose, entry: &SocialAccountEntry, info: &super::actions::InstanceInfo, env: Env, rects: &mut Vec<(String, Rect)>, out: &mut Out) {
    let network = entry.account.network;
    let sending = c.stage == Stage::Sending;
    if let Some(r) = &c.reply {
        ui.add(egui::Label::new(muted(format!("Replying to @{}: {}", r.author.handle, short(&super::markup::plain_text(&r.text_markdown), 120)))).wrap());
        ui.add_space(4.0);
    }
    if let Some(q) = &c.quote {
        inset(ui, |ui| {
            ui.label(muted(format!("Quoting {} @{}", q.author.name(), q.author.handle)));
            ui.add(egui::Label::new(RichText::new(short(&super::markup::plain_text(&q.text_markdown), 160)).size(crate::fonts::px(13.0)).color(theme::palette().text)).wrap());
        });
        if !c.real_quote(network, info) {
            let r = ui.add(egui::Label::new(muted("This server cannot quote posts yet: the web address of the post is added to your text instead.")).wrap());
            rects.push(("compose:quote-note".into(), r.rect));
        }
        ui.add_space(4.0);
    }
    ui.add_enabled_ui(!sending, |ui| {
        match network {
            Network::Mastodon => {
                ui.horizontal(|ui| {
                    let r = crate::clickable::pointer(ui.checkbox(&mut c.cw_on, "Content warning"));
                    rects.push(("compose:cw-toggle".into(), r.rect));
                    let combo = egui::ComboBox::from_id_salt("compose-visibility").selected_text(c.visibility.label()).show_pointer(ui, |ui| {
                        for v in Visibility::ALL {
                            crate::clickable::pointer(ui.selectable_value(&mut c.visibility, v, v.label()));
                        }
                    });
                    rects.push(("compose:visibility".into(), combo.response.rect));
                });
                if c.cw_on {
                    let r = ui.add(egui::TextEdit::singleline(&mut c.cw).hint_text("What the warning says").desired_width(f32::INFINITY));
                    rects.push(("compose:cw".into(), r.rect));
                }
            }
            Network::Bluesky => {
                // Bluesky has no free-text content warnings (only labels for adult or graphic pictures)
                let r = ui.add(egui::Label::new(muted("Bluesky has no content warning for text.")).wrap());
                rects.push(("compose:no-cw".into(), r.rect));
            }
        }
        let te = ui.add(egui::TextEdit::multiline(&mut c.text).desired_rows(6).desired_width(f32::INFINITY).hint_text("What is on your mind?"));
        rects.push(("compose:text".into(), te.rect));
        if c.focus {
            te.request_focus();
            c.focus = false;
        }
        extras::language_picker(ui, c, rects);
    });
    // the counter: characters of the network's own kind against its limit
    let n = c.length(network, info);
    let over = n > info.max_chars;
    ui.horizontal(|ui| {
        if let Some(why) = c.problem(network, info) {
            ui.label(muted(why));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let color = if over { theme::palette().link_missing } else { theme::palette().muted };
            let r = ui.label(RichText::new(format!("{n} / {}", info.max_chars)).size(crate::fonts::px(12.5)).color(color));
            rects.push(("compose:counter".into(), r.rect));
        });
    });
    ui.add_space(4.0);
    if network == Network::Mastodon {
        ui.add_enabled_ui(!sending, |ui| extras::poll_section(ui, c, info, rects));
    }
    // pictures with their descriptions
    let mut remove = None;
    for (i, p) in c.pictures.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            let name = p.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            ui.label(muted(short(&name, 24)));
            let r = ui.add_enabled(!sending, egui::TextEdit::singleline(&mut p.alt).char_limit(1500).hint_text("Describe the picture (alt text)").desired_width(ui.available_width() - 80.0));
            rects.push((format!("compose:alt:{i}"), r.rect));
            let r = ui.add_enabled(!sending, egui::Button::new("Remove").small());
            rects.push((format!("compose:remove:{i}"), r.rect));
            if r.clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        c.remove_picture(i);
    }
    extras::video_row(ui, c, sending, network, rects);
    if c.pictures.len() < info.max_pictures && c.poll.is_none() && c.video.is_none() {
        ui.horizontal(|ui| {
            let b = ui
                .add_enabled(!sending && !env.picking, egui::Button::new(if env.picking { "Choosing…" } else { "Choose pictures or video…" }))
                .on_hover_text("Pick picture or video files in a dialog. You can also drop files on this box or paste a picture (Ctrl+V). A post has up to four pictures or one video.");
            rects.push(("compose:choose".into(), b.rect));
            out.pick |= b.clicked();
            let r = ui.add_enabled(!sending, egui::TextEdit::singleline(&mut c.path).hint_text("or the path of a picture or video").desired_width(ui.available_width() - 110.0));
            rects.push(("compose:path".into(), r.rect));
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            let b = ui.add_enabled(!sending && !c.path.trim().is_empty(), egui::Button::new("Add file"));
            rects.push(("compose:add-picture".into(), b.rect));
            out.add_picture |= b.clicked() || (enter && !c.path.trim().is_empty());
        });
    }
    if network == Network::Mastodon {
        ui.add_enabled_ui(!sending, |ui| extras::schedule_section(ui, c, env.monday, &mut out.scheduled_list, rects));
    }
    if let Some(e) = &c.error {
        let r = ui.add(egui::Label::new(RichText::new(e).color(theme::palette().link_missing)).wrap());
        rects.push(("compose:error".into(), r.rect));
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if sending {
            ui.spinner();
            ui.label(muted("Posting…"));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let ok = !sending && c.problem(network, info).is_none();
            let send = ui.add_enabled(ok, egui::Button::new(RichText::new("Send").strong()).fill(theme::palette().accent.gamma_multiply(0.55))).on_hover_text("Ctrl+Enter. You are asked once more before it is posted.");
            rects.push(("compose:send".into(), send.rect));
            out.ask |= send.clicked();
            let cancel = ui.add_enabled(!sending, egui::Button::new("Cancel")).on_hover_text("Close without posting (Esc)");
            rects.push(("compose:cancel".into(), cancel.rect));
            out.close |= cancel.clicked();
        });
    });
}

/// "Post this?": the account and who may see it, the text, Post and Cancel (SOC-055).
fn confirm(ui: &mut Ui, c: &Compose, entry: &SocialAccountEntry, info: &super::actions::InstanceInfo, rects: &mut Vec<(String, Rect)>, out: &mut Out) {
    let network = entry.account.network;
    ui.label(RichText::new("Post this?").strong().size(crate::fonts::px(16.0)).color(theme::palette().text_strong));
    let who = match network {
        Network::Mastodon => format!("@{} · {}", entry.account.handle, c.visibility.label()),
        Network::Bluesky => format!("@{} · public", entry.account.handle),
    };
    let r = ui.label(muted(who));
    rects.push(("compose:confirm-who".into(), r.rect));
    inset(ui, |ui| {
        if c.cw_on && network == Network::Mastodon && !c.cw.trim().is_empty() {
            ui.label(muted(format!("Content warning: {}", c.cw.trim())));
        }
        ui.add(egui::Label::new(RichText::new(short(&c.final_text(network, info), 400)).size(crate::fonts::px(13.5)).color(theme::palette().text)).wrap());
        if !c.pictures.is_empty() {
            ui.label(muted(format!("{} picture{}", c.pictures.len(), if c.pictures.len() == 1 { "" } else { "s" })));
        }
        if c.video.is_some() {
            ui.label(muted("1 video"));
        }
        if let Some(p) = c.poll.as_ref().filter(|_| network == Network::Mastodon) {
            ui.label(muted(format!("Poll: {}", p.draft().options.join(" · "))));
        }
        if !c.language.is_empty() {
            ui.label(muted(format!("Language: {}", super::language::name(&c.language))));
        }
    });
    if let Some(at) = c.scheduled_at(network) {
        let r = ui.label(RichText::new(format!("Will be sent {} (your local time), not now", super::schedule::when_text(at))).strong().size(crate::fonts::px(13.0)).color(theme::palette().text_strong));
        rects.push(("compose:confirm-schedule".into(), r.rect));
    }
    ui.add_space(6.0);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let post = ui.add(egui::Button::new(RichText::new("Post").strong()).fill(theme::palette().accent.gamma_multiply(0.55)));
        rects.push(("compose:confirm-post".into(), post.rect));
        out.post |= post.clicked();
        let cancel = ui.button("Cancel");
        // Enter or Space on the question presses Post, unless the person moved to Cancel
        if !cancel.has_focus() && !post.has_focus() {
            post.request_focus();
        }
        rects.push(("compose:confirm-cancel".into(), cancel.rect));
        out.back |= cancel.clicked();
    });
}

/// What reaches the box besides typing (SOC-058): the dialog's answer, pictures pasted from the
/// clipboard, files dropped on the box (with a highlight while they hover over it). `rect` is where
/// the box is; the app does not import a drop the box took.
fn pictures_in(ctx: &egui::Context, st: &mut SocialState, c: &mut Compose, info: &super::actions::InstanceInfo, rect: Option<Rect>) {
    st.intake.box_rect = rect;
    if let Some(answer) = st.intake.arrived.take() {
        c.error = match answer {
            Ok(files) => c.attach_files(&files, info).err(),
            Err(e) => Some(e),
        };
    }
    let pointer = ctx.input(|i| i.pointer.latest_pos());
    let over = rect.zip(pointer).is_some_and(|(r, p)| r.contains(p));
    if c.stage != Stage::Edit {
        // while the question or the job is open nothing can be added; a drop on the box is not an import either
        if over && !pictures::dropped_now(ctx).is_empty() {
            st.intake.took_drop = true;
        }
        return;
    }
    for img in pictures::take_pasted(ctx, crate::media::clipboard_image) {
        c.error = c.attach_pasted(&img, info).err();
    }
    let Some(r) = rect else { return };
    let mut dropped = std::mem::take(&mut st.intake.queued);
    if over {
        let now = pictures::dropped_now(ctx);
        if !now.is_empty() {
            st.intake.took_drop = true;
            dropped.extend(now);
        }
    }
    if !dropped.is_empty() {
        c.error = c.attach_files(&dropped, info).err();
    }
    let hovering = (over && ctx.input(|i| !i.raw.hovered_files.is_empty())) || st.intake.ext_hover.is_some_and(|p| r.contains(p));
    if hovering {
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("compose-drop")));
        painter.rect_filled(r, 6.0, theme::palette().accent.gamma_multiply(0.14));
        painter.rect_stroke(r, 6.0, egui::Stroke::new(2.0, theme::palette().accent), egui::StrokeKind::Inside);
        painter.text(r.center_top() + Vec2::new(0.0, 28.0), egui::Align2::CENTER_CENTER, "Drop pictures to add them", egui::FontId::proportional(crate::fonts::px(14.0)), theme::palette().text_strong);
        st.rects.push(("compose:drop-highlight".into(), r));
        ctx.request_repaint();
    }
}
