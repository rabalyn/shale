//! Shale — a native Rust notes vault (MIT licensed).
//!
//! Usage:
//!   shale                       open the last vault (or the start screen)
//!   shale --vault <folder>      open a specific folder as vault
//!   shale --screenshot out.png  start with a demo vault, draw ~120 frames, save a picture, exit
//!   shale --screenshot out.png --view tasks|graph|drawing|reading|calendar|mail   (same, other view)
//!   shale --screenshot out.png --keys 'click:@Sync|waitfor:@Compose'   (scripted input, see script.rs)
//!   shale --screenshot out.png --size 1000x700   (window size; docs/screenshots/regen.sh makes all pictures)
//!
//! An unknown option or a missing value is an error (exit code 2), never a vault folder.

mod app;
mod cal_dav;
mod cal_dialog;
mod cal_ics;
mod cal_store;
mod cal_time;
mod cal_ui;
#[cfg(test)]
mod caldav_env_tests;
mod canvas;
mod compose;
mod config;
mod data_usage;
mod dialog;
mod drawing;
mod drawing_layout;
mod drawing_pictures;
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
mod instance;
mod jobs;
mod keys;
mod keys_ui;
mod layout;
mod link_assist;
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
mod mail_unsent;
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
mod picker;
mod reader;
mod recovery;
mod script;
mod search_all;
mod secret;
mod secrets;
mod sidebar;
#[cfg(test)]
mod sidebar_tests;
mod sidebar_ui;
mod smtp_client;
#[cfg(test)]
mod smtp_fake_tests;
mod social;
#[cfg(test)]
mod tab_order_tests;
mod tagwords;
// Thunderbird's own mail folders, looked up for dropped messages (Wayland, IMP-008)
#[cfg(target_os = "linux")]
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

/// `1360x840` as window points.
fn parse_size(s: &str) -> Option<[f32; 2]> {
    let (w, h) = s.split_once('x')?;
    let (w, h): (f32, f32) = (w.parse().ok()?, h.parse().ok()?);
    (w >= 720.0 && h >= 480.0).then_some([w, h])
}

const USAGE: &str = "usage: shale [<folder> | --vault <folder>] [--screenshot <file.png> [--view <name>] [--size WIDTHxHEIGHT] [--keys <script>]]";

/// What the command line asked for.
#[derive(Debug, PartialEq)]
struct Args {
    screenshot: Option<PathBuf>,
    vault: Option<PathBuf>,
    /// testing aid for screenshots: tasks | graph | drawing | reading | …
    view: Option<String>,
    /// testing aid with --screenshot: scripted input, e.g. "text:[!/|key:Down|key:Tab"
    keys: Option<String>,
    /// testing aid with --screenshot: the window size in points
    size: [f32; 2],
}

/// Read the command line; `None` for `--help`. Anything it does not understand is an error, so
/// the value of an unknown option never becomes a vault folder (BUILD-019).
fn parse_args(it: impl IntoIterator<Item = String>) -> Result<Option<Args>, String> {
    let mut a = Args { screenshot: None, vault: None, view: None, keys: None, size: [1360.0, 840.0] };
    let mut it = it.into_iter();
    while let Some(opt) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{opt} needs a value"));
        match opt.as_str() {
            "--screenshot" => a.screenshot = Some(PathBuf::from(value()?)),
            "--vault" => a.vault = Some(PathBuf::from(value()?)),
            "--view" => a.view = Some(value()?),
            "--keys" => a.keys = Some(value()?),
            "--size" => {
                let v = value()?;
                a.size = parse_size(&v).ok_or_else(|| format!("--size needs WIDTHxHEIGHT (at least 720x480), e.g. 1360x840, not {v}"))?;
            }
            "-h" | "--help" => return Ok(None),
            o if o.starts_with('-') => return Err(format!("unknown option {o}")),
            folder => match &a.vault {
                None => a.vault = Some(PathBuf::from(folder)),
                Some(v) => return Err(format!("one vault folder only, got {} and {folder}", v.display())),
            },
        }
    }
    Ok(Some(a))
}

fn main() -> eframe::Result {
    // release builds: no core dumps, no memory reads by other processes (SEC-019)
    if let Err(e) = secret::harden_process(cfg!(not(debug_assertions))) {
        eprintln!("shale: could not make the process undumpable: {e}");
    }
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(Some(a)) => a,
        Ok(None) => {
            println!("{USAGE}");
            return Ok(());
        }
        Err(e) => {
            eprintln!("shale: {e}\n{USAGE}");
            std::process::exit(2);
        }
    };
    let Args { screenshot, vault, view, keys, size } = args;

    if let Err(e) = script::parse(keys.as_deref().unwrap_or("")) {
        eprintln!("--keys: {e}");
        std::process::exit(2);
    }

    // Not for screenshots: they use their own temporary folders and may run beside the real app.
    let _instance_lock = if screenshot.is_none() {
        match instance::acquire(&instance::default_dir()) {
            Ok(Some(lock)) => Some(lock),
            Ok(None) => {
                instance::show_already_running();
                std::process::exit(1);
            }
            Err(e) => {
                // a broken data folder must not lock the person out of their notes
                eprintln!("Shale could not check whether it is already running ({e}); starting anyway.");
                None
            }
        }
    } else {
        None
    };

    let mut viewport = egui::ViewportBuilder::default().with_title("Shale").with_app_id("shale").with_inner_size(size).with_min_inner_size([720.0, 480.0]);
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

#[cfg(test)]
mod tests {
    use super::{parse_args, parse_size};
    use std::path::PathBuf;

    fn args(s: &[&str]) -> Result<Option<super::Args>, String> {
        parse_args(s.iter().map(|a| a.to_string()))
    }

    // spec: BUILD-019 (#85: an old build skipped --size and opened "1360x840" as a new vault)
    #[test]
    fn an_unknown_option_is_an_error_not_a_vault() {
        assert_eq!(args(&["--sise", "1360x840"]).unwrap_err(), "unknown option --sise");
        assert_eq!(args(&["-x"]).unwrap_err(), "unknown option -x");
        assert_eq!(args(&["notes", "1360x840"]).unwrap_err(), "one vault folder only, got notes and 1360x840");
        assert_eq!(args(&["--vault", "a", "b"]).unwrap_err(), "one vault folder only, got a and b");
        assert!(args(&["--size", "1360"]).unwrap_err().contains("not 1360"));
    }

    // spec: BUILD-019
    #[test]
    fn an_option_needs_its_value() {
        for o in ["--screenshot", "--vault", "--view", "--keys", "--size"] {
            assert_eq!(args(&[o]).unwrap_err(), format!("{o} needs a value"));
        }
    }

    // spec: BUILD-019, VAULT-006
    #[test]
    fn one_folder_and_known_options_parse() {
        assert_eq!(args(&["--help"]), Ok(None));
        assert_eq!(args(&["-h", "--bogus"]), Ok(None));
        let a = args(&[]).unwrap().unwrap();
        assert_eq!((a.vault, a.screenshot, a.size), (None, None, [1360.0, 840.0]));
        let a = args(&["My Notes"]).unwrap().unwrap();
        assert_eq!(a.vault, Some(PathBuf::from("My Notes")));
        let a = args(&["--screenshot", "o.png", "--view", "mail", "--size", "1000x700", "--keys", "key:Tab", "--vault", "v"]).unwrap().unwrap();
        assert_eq!(a.screenshot, Some(PathBuf::from("o.png")));
        assert_eq!(a.vault, Some(PathBuf::from("v")));
        assert_eq!((a.view.as_deref(), a.keys.as_deref(), a.size), (Some("mail"), Some("key:Tab"), [1000.0, 700.0]));
    }

    // spec: DEV-007
    #[test]
    fn a_window_size_is_width_x_height_and_not_tiny() {
        assert_eq!(parse_size("1360x840"), Some([1360.0, 840.0]));
        assert_eq!(parse_size("100x100"), None);
        assert_eq!(parse_size("1360"), None);
        assert_eq!(parse_size("axb"), None);
    }
}
