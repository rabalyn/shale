//! Shale — a native Rust notes vault (MIT licensed).
//!
//! Usage:
//!   shale                       open the last vault (or the start screen)
//!   shale --vault <folder>      open a specific folder as vault
//!   shale --screenshot out.png  start with a demo vault, draw ~120 frames, save a picture, exit
//!   shale --screenshot out.png --view tasks|graph|drawing|reading|calendar|mail   (same, other view)

mod app;
mod cal_dav;
mod cal_dialog;
mod cal_ics;
mod cal_store;
mod cal_time;
mod cal_ui;
mod compose;
mod config;
mod drawing;
mod drawing_layout;
mod editor;
mod excalidraw_md;
mod export_dialog;
mod fsutil;
mod gitsync;
mod gpg;
mod graph;
mod imap_client;
#[cfg(test)]
mod imap_fake_tests;
mod imap_sync;
mod import;
mod import_dialog;
mod jobs;
mod layout;
mod lists;
mod mail;
mod mail_compose;
mod mail_crypto;
mod mail_db;
mod mail_demo;
mod mail_dialog;
#[cfg(test)]
mod mail_env_tests;
mod mail_ops;
mod mail_send;
#[cfg(test)]
mod mail_send_fuzz_tests;
mod mail_store;
mod mail_ui;
#[cfg(test)]
mod mail_ui_tests;
mod mail_view;
mod markdown;
mod media;
#[cfg(test)]
mod notes_vault_tests;
mod oauth;
#[cfg(test)]
mod oauth_fake_tests;
mod opener;
mod outbox;
mod panels;
mod pdfexport;
mod pdfview;
mod pdfwrite;
mod people;
mod reader;
mod recovery;
mod secrets;
mod smtp_client;
#[cfg(test)]
mod smtp_fake_tests;
mod tagwords;
mod tb_store;
mod theme;
#[cfg(test)]
mod utf8_tests;
mod vault;
mod vault_path;
mod vault_state;
mod vim;
#[cfg(target_os = "linux")]
mod wl_drop;

use eframe::egui;
use std::path::PathBuf;

fn main() -> eframe::Result {
    let mut screenshot: Option<PathBuf> = None;
    let mut vault: Option<PathBuf> = None;
    let mut view: Option<String> = None;
    let mut keys: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--screenshot" => screenshot = args.next().map(PathBuf::from),
            "--vault" => vault = args.next().map(PathBuf::from),
            // testing aid for screenshots: tasks | graph | drawing | reading
            "--view" => view = args.next(),
            // testing aid with --screenshot: scripted typing, e.g. "text:[!/|key:Down|key:Tab"
            "--keys" => keys = args.next(),
            "-h" | "--help" => {
                println!("shale [--vault <folder>] [--screenshot <file.png>]");
                return Ok(());
            }
            other if !other.starts_with('-') && vault.is_none() => vault = Some(PathBuf::from(other)),
            _ => {}
        }
    }

    let mut viewport = egui::ViewportBuilder::default().with_title("Shale").with_app_id("shale").with_inner_size([1360.0, 840.0]).with_min_inner_size([720.0, 480.0]);
    if screenshot.is_some() {
        // Don't steal focus from whatever the person is doing.
        viewport = viewport.with_active(false);
        #[cfg(target_os = "windows")]
        {
            viewport = viewport.with_window_level(egui::WindowLevel::AlwaysOnBottom);
        }
    }
    #[allow(unused_mut)]
    let mut options = eframe::NativeOptions { viewport, ..Default::default() };
    #[cfg(target_os = "macos")]
    if screenshot.is_some() {
        use winit::platform::macos::EventLoopBuilderExtMacOS;
        options.event_loop_builder = Some(Box::new(|b| {
            b.with_activate_ignoring_other_apps(false);
        }));
    }
    let opts = app::Options { screenshot, vault, view, keys };
    #[cfg(target_os = "linux")]
    {
        // Our own event loop, so drops from other programs can be listened for on Wayland
        // before the clipboard helper starts (see wl_drop).
        let event_loop = winit::event_loop::EventLoop::<eframe::UserEvent>::with_user_event().build().map_err(eframe::Error::WinitEventLoop)?;
        wl_drop::attach(&event_loop);
        let mut app = eframe::create_native("Shale", options, Box::new(move |cc| Ok(Box::new(app::ShaleApp::new(cc, opts)))), &event_loop);
        event_loop.run_app(&mut app).map_err(eframe::Error::WinitEventLoop)
    }
    #[cfg(not(target_os = "linux"))]
    eframe::run_native("Shale", options, Box::new(move |cc| Ok(Box::new(app::ShaleApp::new(cc, opts)))))
}
