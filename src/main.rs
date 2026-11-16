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
#[cfg(test)]
mod cal_dav_status_tests;
mod cal_dialog;
mod cal_ics;
mod cal_store;
#[cfg(test)]
mod cal_store_failure_tests;
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
mod mail_keys;
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
mod start_dialog_tests;
#[cfg(test)]
mod tab_order_tests;
mod tagwords;
#[cfg(all(test, target_os = "linux"))]
mod tb_fixture_tests;
// Thunderbird's own mail folders, looked up for dropped messages (Wayland, IMP-008)
#[cfg(target_os = "linux")]
mod tb_store;
mod theme;
#[cfg(test)]
mod ui_test_support;
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
struct Cli {
    screenshot: Option<PathBuf>,
    vault: Option<PathBuf>,
    /// testing aid for screenshots: tasks | graph | drawing | reading | …
    view: Option<String>,
    /// testing aid with --screenshot: scripted input, e.g. "text:[!/|key:Down|key:Tab"
    keys: Option<String>,
    /// testing aid with --screenshot: the window size in points
    size: [f32; 2],
}

/// The result of reading the command line.
#[derive(Debug, PartialEq)]
enum Parsed {
    Run(Cli),
    /// `-h`: print the usage and end with success
    Help,
    /// something that cannot be used: the message for stderr; the exit code is 2
    Bad(String),
}

/// Read the arguments after the program name (DEV-013), without any output or exit. Anything it
/// does not understand is an error, so the value of an unknown option never becomes a vault
/// folder (BUILD-019); a bare word is the vault folder, and only one is allowed.
fn parse_args(args: impl IntoIterator<Item = String>) -> Parsed {
    read_args(args).unwrap_or_else(Parsed::Bad)
}

fn read_args(args: impl IntoIterator<Item = String>) -> Result<Parsed, String> {
    let mut cli = Cli { screenshot: None, vault: None, view: None, keys: None, size: [1360.0, 840.0] };
    let mut args = args.into_iter();
    while let Some(opt) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{opt} needs a value"));
        match opt.as_str() {
            "--screenshot" => cli.screenshot = Some(PathBuf::from(value()?)),
            "--vault" => cli.vault = Some(PathBuf::from(value()?)),
            "--view" => cli.view = Some(value()?),
            "--keys" => cli.keys = Some(value()?),
            "--size" => {
                let v = value()?;
                cli.size = parse_size(&v).ok_or_else(|| format!("--size needs WIDTHxHEIGHT (at least 720x480), e.g. 1360x840, not {v}"))?;
            }
            "-h" | "--help" => return Ok(Parsed::Help),
            o if o.starts_with('-') => return Err(format!("unknown option {o}")),
            folder => match &cli.vault {
                None => cli.vault = Some(PathBuf::from(folder)),
                Some(v) => return Err(format!("one vault folder only, got {} and {folder}", v.display())),
            },
        }
    }
    script::parse(cli.keys.as_deref().unwrap_or("")).map_err(|e| format!("--keys: {e}"))?;
    Ok(Parsed::Run(cli))
}

fn main() -> eframe::Result {
    // release builds: no core dumps, no memory reads by other processes (SEC-019)
    if let Err(e) = secret::harden_process(cfg!(not(debug_assertions))) {
        eprintln!("shale: could not make the process undumpable: {e}");
    }
    let Cli { screenshot, vault, view, keys, size } = match parse_args(std::env::args().skip(1)) {
        Parsed::Run(cli) => cli,
        Parsed::Help => {
            println!("{USAGE}");
            return Ok(());
        }
        Parsed::Bad(message) => {
            eprintln!("shale: {message}\n{USAGE}");
            std::process::exit(2);
        }
    };

    // Not for screenshots: they use their own temporary folders and may run beside the real app.
    let _instance_lock = if screenshot.is_none() {
        match instance::claim(&instance::default_dir()) {
            instance::Claim::Owner(lock) => Some(lock),
            instance::Claim::AlreadyRunning => {
                instance::show_already_running();
                std::process::exit(1);
            }
            instance::Claim::Unchecked(e) => {
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
    #[cfg(not(target_os = "macos"))]
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    #[cfg(target_os = "macos")]
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
    use super::{parse_args, parse_size, Cli, Parsed};
    use std::path::PathBuf;

    fn parse(args: &[&str]) -> Parsed {
        parse_args(args.iter().map(|s| s.to_string()))
    }

    fn cli(p: Parsed) -> Cli {
        match p {
            Parsed::Run(c) => c,
            other => panic!("{other:?}"),
        }
    }

    fn bad(args: &[&str]) -> String {
        match parse(args) {
            Parsed::Bad(m) => m,
            other => panic!("{args:?}: {other:?}"),
        }
    }

    // spec: BUILD-019 (#85: an old build skipped --size and opened "1360x840" as a new vault)
    #[test]
    fn an_unknown_option_is_an_error_not_a_vault() {
        assert_eq!(bad(&["--sise", "1360x840"]), "unknown option --sise");
        assert_eq!(bad(&["-x"]), "unknown option -x");
        assert_eq!(bad(&["notes", "1360x840"]), "one vault folder only, got notes and 1360x840");
        assert_eq!(bad(&["--vault", "a", "b"]), "one vault folder only, got a and b");
        assert!(bad(&["--size", "1360"]).contains("not 1360"));
    }

    // spec: BUILD-019
    #[test]
    fn an_option_needs_its_value() {
        for o in ["--screenshot", "--vault", "--view", "--keys", "--size"] {
            assert_eq!(bad(&[o]), format!("{o} needs a value"));
        }
    }

    // spec: DEV-007
    #[test]
    fn a_window_size_is_width_x_height_and_not_tiny() {
        assert_eq!(parse_size("1360x840"), Some([1360.0, 840.0]));
        assert_eq!(parse_size("100x100"), None);
        assert_eq!(parse_size("1360"), None);
        assert_eq!(parse_size("axb"), None);
    }

    // spec: DEV-013
    #[test]
    fn no_arguments_start_with_the_defaults() {
        assert_eq!(cli(parse(&[])), Cli { screenshot: None, vault: None, view: None, keys: None, size: [1360.0, 840.0] });
    }

    // spec: DEV-013
    #[test]
    fn every_option_is_read() {
        let c = cli(parse(&["--vault", "/v", "--screenshot", "out.png", "--view", "tasks", "--keys", "key:Down|key:Tab", "--size", "1000x700"]));
        assert_eq!(c, Cli { screenshot: Some("out.png".into()), vault: Some("/v".into()), view: Some("tasks".into()), keys: Some("key:Down|key:Tab".into()), size: [1000.0, 700.0] });
    }

    // spec: DEV-013, BUILD-019
    #[test]
    fn a_bare_word_is_the_vault_and_a_second_one_is_an_error() {
        assert_eq!(cli(parse(&["/notes"])).vault, Some(PathBuf::from("/notes")));
        assert_eq!(cli(parse(&["/b", "--vault", "/a"])).vault, Some(PathBuf::from("/a")), "--vault after a bare word wins");
        assert_eq!(bad(&["--vault", "/a", "/b"]), "one vault folder only, got /a and /b");
        assert_eq!(bad(&["/a", "/b"]), "one vault folder only, got /a and /b");
    }

    // spec: DEV-013, BUILD-019
    #[test]
    fn unknown_options_and_missing_values_are_errors() {
        assert_eq!(bad(&["--nonsense", "--vault"]), "unknown option --nonsense");
        assert_eq!(bad(&["--vault"]), "--vault needs a value");
        assert_eq!(bad(&["--screenshot"]), "--screenshot needs a value");
        assert_eq!(bad(&["-x", "--view"]), "unknown option -x");
    }

    // spec: DEV-013
    #[test]
    fn help_ends_with_success_before_anything_else_is_judged() {
        assert_eq!(parse(&["-h"]), Parsed::Help);
        assert_eq!(parse(&["--vault", "/v", "--help"]), Parsed::Help);
        assert_eq!(parse(&["--help", "--size", "bad"]), Parsed::Help);
    }

    // spec: DEV-013
    #[test]
    fn a_bad_size_is_an_error_for_exit_code_2() {
        for bad_args in [&["--size", "100x100"][..], &["--size", "1360"], &["--size", "axb"], &["--size"]] {
            let m = self::bad(bad_args);
            assert!(m.contains("--size needs"), "{m}");
        }
        assert_eq!(cli(parse(&["--size", "720x480"])).size, [720.0, 480.0], "the smallest window is allowed");
    }

    // spec: DEV-013
    #[test]
    fn bad_keys_are_an_error_for_exit_code_2() {
        match parse(&["--screenshot", "o.png", "--keys", "nonsense:thing"]) {
            Parsed::Bad(m) => assert!(m.starts_with("--keys: "), "{m}"),
            other => panic!("{other:?}"),
        }
        assert!(matches!(parse(&["--keys", "key:Down|click:@Sync"]), Parsed::Run(_)));
        assert!(matches!(parse(&["--keys", ""]), Parsed::Run(_)));
    }
}
