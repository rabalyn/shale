//! The worst case (BUILD-031): one person who uses everything at once. A big vault, Word files
//! imported into it, a big inbox, a big calendar, social feeds with thousands of posts, a vault
//! that is a git repository, and the leftovers of a crash waiting in the recovery folder, all
//! loaded in one window. The scenarios measure start-up, idle and typing frames, a round through
//! every view, and how much memory 2000 frames leave behind (`retained_bytes`).

use super::runner::Skipped;
use super::scen_data::{calendar_source, fill_mail};
use super::scen_features::{crash_leftovers, Cleanup};
use super::scenarios::{wheel, Body, Fixtures, Rig, Scenario};
use super::vault_gen::Size;
use crate::app::{RightPanel, View};
use eframe::egui::Event;
use std::hint::black_box;
use std::path::{Path, PathBuf};

/// The files of the world, made once.
pub struct WorldData {
    vault: PathBuf,
    long_note: PathBuf,
    mail: PathBuf,
    calendar: crate::calendar::store::CalendarSource,
    _leftovers: Cleanup,
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("a folder");
    for e in std::fs::read_dir(from).expect("a folder to copy").flatten() {
        let t = to.join(e.file_name());
        if e.file_type().is_ok_and(|k| k.is_dir()) {
            copy_tree(&e.path(), &t);
        } else {
            std::fs::copy(e.path(), &t).expect("a file is copied");
        }
    }
}

fn build_world(fx: &mut Fixtures) -> WorldData {
    let g = fx.vault(Size::Large);
    let vault = fx.scratch("worst-vault");
    copy_tree(&g.root, &vault);
    match super::scen_io::docx_files(fx).and_then(|files| super::scen_io::import_docx(&vault, &files)) {
        Ok(_) => {}
        Err(why) => fx.skipped.push(Skipped { id: "worst.world.docx".into(), reason: format!("the Word notes are left out of the world: {why}") }),
    }
    let mail = fx.scratch("worst-mail");
    fill_mail(&mail, fx.scale.mail).expect("the inbox is filled");
    let cal_dir = fx.scratch("worst-calendar");
    let calendar = calendar_source(&cal_dir, fx.scale.events);
    // a vault that is a git repository, with a remote: what the sync settings leave behind
    let git_ok = super::scen_io::connect_repository(&vault);
    if let Err(why) = git_ok {
        fx.skipped.push(Skipped { id: "worst.world.git".into(), reason: format!("the vault is not a git repository in the world: {why}") });
    }
    let leftovers = crash_leftovers(&vault, fx.scale.recovery_notes);
    WorldData { vault, long_note: g.long_note, mail, calendar, _leftovers: leftovers }
}

/// The window of the world: everything loaded, the long note open with the backlinks beside it.
fn assemble(fx: &mut Fixtures) -> Rig {
    if fx.world.is_none() {
        let w = build_world(fx);
        fx.world = Some(w);
    }
    let w = fx.world.as_ref().expect("the world is made");
    let (vault, long_note, mail, calendar) = (w.vault.clone(), w.long_note.clone(), w.mail.clone(), w.calendar.clone());
    let mut rig = Rig::at(&vault);
    rig.app.cfg.mail_accounts = vec![crate::config::MailAccount { id: crate::mail::demo::ACCOUNT.into(), name: "Bench mail".into(), host: "imap.example.com".into(), user: "ada@example.com".into(), ..Default::default() }];
    rig.app.cfg.mail_sync_minutes = 0;
    rig.app.mail.root = mail;
    rig.app.cfg.calendars = vec![calendar];
    rig.app.calendar.set_sources(rig.app.cfg.calendars.clone());
    let demo = crate::social::demo::account();
    let id = demo.account.id.clone();
    rig.app.cfg.social_accounts = vec![demo.clone()];
    rig.app.social.load_demo(&demo);
    if let Some(t) = rig.app.social.timelines.get_mut(&(id, "home".into())) {
        t.posts = super::scen_data::posts(fx.scale.posts);
    }
    rig.app.side.layout.show_panel(RightPanel::Backlinks);
    // the mail store opens in a job; the window should have the inbox ready like a person's has after a minute
    rig.app.show_view(View::Mail, false);
    let _ = rig.wait("opening the mail store", |app| app.mail.ready_store().is_some());
    rig.run_for(300);
    rig.app.open(&long_note, false);
    rig.fr.settle(&mut rig.app, 6);
    rig.assert_view(&View::Note(long_note.clone()));
    rig
}

fn startup(fx: &mut Fixtures) -> Body {
    // the world's files are made now (not measured); each iteration starts the window on them
    drop(assemble(fx));
    let world = fx.world.take();
    let mut holder = Fixtures::new(fx.scale);
    holder.world = world;
    Box::new(move || {
        black_box(assemble(&mut holder));
    })
}

fn idle(fx: &mut Fixtures) -> Body {
    let mut rig = assemble(fx);
    Box::new(move || rig.frame(vec![]))
}

fn typing(fx: &mut Fixtures) -> Body {
    let mut rig = assemble(fx);
    let note = fx.world.as_ref().expect("the world").long_note.clone();
    rig.cursor_in_the_middle(&note);
    Box::new(move || rig.frame(vec![Event::Text("x".into())]))
}

fn views(rig: &mut Rig, note: &Path) {
    for v in [View::Graph, View::Tasks, View::Mail, View::Calendar, View::Social(None)] {
        rig.app.show_view(v, false);
        rig.fr.settle(&mut rig.app, 2);
    }
    rig.app.open(note, false);
    rig.fr.settle(&mut rig.app, 2);
    rig.assert_view(&View::Note(note.to_path_buf()));
}

fn view_cycle(fx: &mut Fixtures) -> Body {
    let mut rig = assemble(fx);
    let note = fx.world.as_ref().expect("the world").long_note.clone();
    Box::new(move || views(&mut rig, &note))
}

fn memory_growth(fx: &mut Fixtures) -> Body {
    let mut rig = assemble(fx);
    let note = fx.world.as_ref().expect("the world").long_note.clone();
    rig.cursor_in_the_middle(&note);
    let frames = fx.scale.leak_frames;
    Box::new(move || {
        for i in 0..frames {
            match i % 4 {
                0 => rig.frame(vec![Event::Text("x".into())]),
                1 => rig.frame(vec![wheel(-120.0)]),
                _ => rig.frame(vec![]),
            }
            if i % 100 == 99 {
                views(&mut rig, &note);
                rig.cursor_in_the_middle(&note);
            }
        }
    })
}

pub fn list() -> Vec<Scenario> {
    let p = Scenario::plain;
    vec![p("worst.startup", 3, false, startup).cold(), p("worst.frame.idle", 60, false, idle), p("worst.frame.typing", 30, false, typing), p("worst.views.cycle", 3, false, view_cycle), p("worst.memory.growth", 1, false, memory_growth)]
}
