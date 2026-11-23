//! Pictures coming into the compose box other ways than a typed path (SOC-058): the file dialog
//! (a pretend one, nothing opens), files dropped on the box, a picture pasted from the clipboard.

use super::actions::tests::png;
use super::compose::Compose;
use super::compose_pictures::{take_pasted, Picker};
use super::ui_tests::{wait_for, W};
use crate::app::Modal;
use eframe::egui::{self, Event, Pos2};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// A dialog that answers what it is told (one answer per call), after the gate when there is one.
#[derive(Default)]
struct FakePicker {
    answers: Mutex<Vec<Result<Vec<PathBuf>, String>>>,
    asked: Mutex<Vec<usize>>,
    gate: Mutex<Option<std::sync::mpsc::Receiver<()>>>,
}

impl Picker for FakePicker {
    fn pick(&self, max: usize) -> Result<Vec<PathBuf>, String> {
        self.asked.lock().unwrap().push(max);
        if let Some(rx) = self.gate.lock().unwrap().as_ref() {
            let _ = rx.recv();
        }
        let mut q = self.answers.lock().unwrap();
        if q.is_empty() {
            Ok(vec![])
        } else {
            q.remove(0)
        }
    }
}

fn compose(w: &W) -> &Compose {
    w.app.social.compose.as_ref().expect("the compose box is open")
}

fn picture(w: &W, name: &str) -> PathBuf {
    let p = w.dir.join(name);
    std::fs::write(&p, png(20, 20, false)).unwrap();
    p
}

fn pretend_dialog(w: &mut W) -> Arc<FakePicker> {
    let f = Arc::new(FakePicker::default());
    w.app.social.intake.picker = f.clone();
    f
}

/// Files dropped on the window with the pointer at `at`.
fn drop_at(w: &mut W, at: Pos2, files: &[PathBuf]) {
    w.frame(vec![Event::PointerMoved(at)]);
    let files = files.to_vec();
    w.frame_raw(vec![], move |raw| raw.dropped_files.extend(files.into_iter().map(|p| Arc::new(crate::app::ScriptDrop(p)) as _)));
    w.frames(2);
}

fn tiny_image() -> egui::ColorImage {
    egui::ColorImage::filled([8, 6], egui::Color32::from_rgb(200, 30, 30))
}

// spec: SOC-058
#[test]
fn soc_058_the_dialog_runs_beside_the_frames_and_its_pictures_join_the_post_up_to_the_limit() {
    let mut w = W::new("pick-dialog");
    let dialog = pretend_dialog(&mut w);
    let files: Vec<PathBuf> = ["a.png", "b.png", "c.png", "d.png", "e.png"].iter().map(|n| picture(&w, n)).collect();
    let (tx, rx) = std::sync::mpsc::channel();
    *dialog.gate.lock().unwrap() = Some(rx);
    dialog.answers.lock().unwrap().push(Ok(files.clone()));
    w.click("new-post");
    w.click("compose:choose");
    // the dialog is open (it waits at its gate) and the window keeps running: no second dialog, no picture yet
    let seen = dialog.clone();
    wait_for(&mut w, move |_| !seen.asked.lock().unwrap().is_empty());
    w.frames(3);
    assert_eq!(*dialog.asked.lock().unwrap(), [4], "asked for at most the pictures that still fit");
    assert!(w.app.social.intake.picking && compose(&w).pictures.is_empty());
    w.click("compose:choose");
    assert_eq!(dialog.asked.lock().unwrap().len(), 1, "a dialog that is open is not opened again");
    tx.send(()).unwrap();
    wait_for(&mut w, |w| w.app.social.compose.as_ref().is_some_and(|c| !c.pictures.is_empty()));
    // five came back, four fit: the post has four, in order, and the box says what did not fit
    assert_eq!(compose(&w).pictures.iter().map(|p| p.path.clone()).collect::<Vec<_>>(), files[..4].to_vec());
    assert!(compose(&w).error.as_deref().is_some_and(|e| e.contains("at most 4 pictures")), "{:?}", compose(&w).error);
    assert!(!w.app.social.intake.picking);
    w.frames(2);
    assert!(!w.app.social.rects.iter().any(|r| r.0 == "compose:choose"), "no button for a fifth picture");
}

// spec: SOC-058
#[test]
fn soc_058_a_cancelled_dialog_changes_nothing_and_a_chosen_non_picture_is_refused() {
    let mut w = W::new("pick-cancel");
    let dialog = pretend_dialog(&mut w);
    let words = w.dir.join("words.png");
    std::fs::write(&words, "no picture").unwrap();
    w.click("new-post");
    w.click("compose:choose");
    wait_for(&mut w, |w| !w.app.social.intake.picking);
    assert!(compose(&w).pictures.is_empty() && compose(&w).error.is_none(), "cancel says nothing");
    dialog.answers.lock().unwrap().push(Ok(vec![words, picture(&w, "ok.png")]));
    w.click("compose:choose");
    wait_for(&mut w, |w| !compose(w).pictures.is_empty());
    assert_eq!(compose(&w).pictures.len(), 1);
    assert!(compose(&w).error.as_deref().is_some_and(|e| e.contains("“words.png” is not a JPEG, PNG, GIF or WebP picture")), "{:?}", compose(&w).error);
}

// spec: SOC-058
#[test]
fn soc_058_a_dialog_that_cannot_open_says_so_and_the_typed_path_still_works() {
    let mut w = W::new("pick-error");
    let dialog = pretend_dialog(&mut w);
    dialog.answers.lock().unwrap().push(Err("No file dialog could be opened here (it needs the xdg-desktop-portal). Type the path of the picture instead.".into()));
    let a = picture(&w, "a.png");
    w.click("new-post");
    w.click("compose:choose");
    wait_for(&mut w, |w| compose(w).error.is_some());
    assert!(compose(&w).error.as_deref().unwrap().contains("xdg-desktop-portal"));
    assert!(!w.app.social.intake.picking, "the button works again");
    w.app.social.compose.as_mut().unwrap().path = a.display().to_string();
    w.frames(2);
    w.click("compose:add-picture");
    assert_eq!((compose(&w).pictures.len(), compose(&w).error.clone()), (1, None));
}

// spec: SOC-058
#[test]
fn soc_058_pictures_dropped_on_the_box_join_the_post_with_a_highlight_and_refuse_other_files() {
    let mut w = W::new("drop-on-box");
    let (png1, png2) = (picture(&w, "one.png"), picture(&w, "two.png"));
    let text = w.dir.join("notes.txt");
    std::fs::write(&text, "words").unwrap();
    w.click("new-post");
    let inside = w.wait_rect("compose:text").center();
    // a drag over the box lights it up
    w.frame(vec![Event::PointerMoved(inside)]);
    w.frame_raw(vec![], |raw| raw.hovered_files.push(egui::HoveredFile::default()));
    assert!(w.app.social.rects.iter().any(|r| r.0 == "compose:drop-highlight"), "the box is highlighted");
    w.frames(2);
    assert!(!w.app.social.rects.iter().any(|r| r.0 == "compose:drop-highlight"), "and not any more when the drag has left");
    // two pictures and a text file: the pictures are added, the other file is named in the message
    drop_at(&mut w, inside, &[png1.clone(), text, png2.clone()]);
    assert_eq!(compose(&w).pictures.iter().map(|p| p.path.clone()).collect::<Vec<_>>(), vec![png1, png2]);
    assert!(compose(&w).error.as_deref().is_some_and(|e| e.contains("“notes.txt” is not a JPEG, PNG, GIF or WebP picture")), "{:?}", compose(&w).error);
    // a drop that brings only a refused file adds nothing
    let only = w.dir.join("only.txt");
    std::fs::write(&only, "x").unwrap();
    drop_at(&mut w, inside, &[only]);
    assert_eq!(compose(&w).pictures.len(), 2);
    assert!(compose(&w).error.is_some());
}

// spec: SOC-058
#[test]
fn soc_058_a_drop_on_the_box_is_not_imported_into_the_vault_but_one_beside_it_is() {
    let mut w = W::new("drop-no-import");
    let note = w.dir.join("Dropped.md");
    std::fs::write(&note, "# Dropped\n").unwrap();
    let pic = picture(&w, "p.png");
    w.click("new-post");
    let inside = w.wait_rect("compose:text").center();
    drop_at(&mut w, inside, &[pic]);
    assert_eq!(compose(&w).pictures.len(), 1);
    assert!(!matches!(w.app.modal, Some(Modal::Import(_))), "the drop on the box is no import");
    // a file dropped on the box that is no picture is not imported either (it is refused with a message)
    drop_at(&mut w, inside, std::slice::from_ref(&note));
    assert!(!matches!(w.app.modal, Some(Modal::Import(_))));
    assert!(compose(&w).error.is_some());
    // beside the box the same drop opens the Import dialog as before
    let beside = Pos2::new(10.0, 880.0);
    assert!(!w.app.social.intake.over(beside));
    drop_at(&mut w, beside, &[note]);
    assert!(matches!(w.app.modal, Some(Modal::Import(_))), "{:?}", w.app.modal.is_some());
}

// spec: SOC-058
#[test]
fn soc_058_files_dropped_from_another_program_are_queued_for_the_box_and_added() {
    let mut w = W::new("drop-queued");
    let pic = picture(&w, "q.png");
    w.click("new-post");
    w.app.social.intake.queued.push(pic.clone());
    w.frames(2);
    assert_eq!(compose(&w).pictures.iter().map(|p| p.path.clone()).collect::<Vec<_>>(), vec![pic]);
    assert!(w.app.social.intake.queued.is_empty());
}

// spec: SOC-058
#[test]
fn soc_058_a_pasted_picture_becomes_a_private_temporary_file_that_goes_with_the_picture() {
    let mut w = W::new("paste-image");
    w.click("new-post");
    w.frame(vec![Event::PasteImage(Arc::new(tiny_image()))]);
    w.frames(2);
    assert_eq!(compose(&w).pictures.len(), 1);
    let file = compose(&w).pictures[0].path.clone();
    assert!(file.starts_with(crate::fsutil::private_temp_root("shale-compose").unwrap()), "{file:?}");
    assert_eq!(image::guess_format(&std::fs::read(&file).unwrap()).unwrap(), image::ImageFormat::Png);
    assert_eq!(compose(&w).problem(super::Network::Mastodon, &super::actions::InstanceInfo::defaults(super::Network::Mastodon)), None, "a picture alone is a post");
    // removed: the file is gone
    w.click("compose:remove:0");
    assert!(compose(&w).pictures.is_empty() && !file.exists());
    // pasted again and the box closed with Cancel: the file is gone too
    w.frame(vec![Event::PasteImage(Arc::new(tiny_image()))]);
    w.frames(2);
    let file = compose(&w).pictures[0].path.clone();
    assert!(file.exists());
    w.click("compose:cancel");
    assert!(w.app.social.compose.is_none() && !file.exists());
}

// spec: SOC-058
#[test]
fn soc_058_a_pasted_picture_counts_against_the_limit_of_four() {
    let mut w = W::new("paste-limit");
    w.click("new-post");
    for _ in 0..4 {
        w.frame(vec![Event::PasteImage(Arc::new(tiny_image()))]);
        w.frames(2);
    }
    assert_eq!((compose(&w).pictures.len(), compose(&w).error.clone()), (4, None));
    let files: Vec<PathBuf> = compose(&w).pictures.iter().map(|p| p.path.clone()).collect();
    w.frame(vec![Event::PasteImage(Arc::new(tiny_image()))]);
    w.frames(2);
    assert_eq!(compose(&w).pictures.len(), 4);
    assert!(compose(&w).error.as_deref().is_some_and(|e| e.contains("at most 4 pictures")));
    // nothing is left over from the refused one but the four
    let dir = crate::fsutil::private_temp_root("shale-compose").unwrap();
    let ours = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.path()).filter(|p| files.contains(p)).count();
    assert_eq!(ours, 4);
}

// spec: SOC-058
#[test]
fn soc_058_an_img_tag_pasted_as_text_is_the_clipboards_picture_when_it_has_one() {
    let ctx = egui::Context::default();
    let tag = r#"<img src="https://example.org/a.png">"#;
    let run = |has_picture: bool| {
        let mut taken = 0;
        let mut kept = 0;
        let raw = egui::RawInput { events: vec![Event::Paste(tag.into()), Event::Paste("plain words".into())], ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| {
            taken = take_pasted(ui.ctx(), || has_picture.then(tiny_image)).len();
            kept = ui.ctx().input(|i| i.events.len());
        });
        out.textures_delta.clear();
        (taken, kept)
    };
    assert_eq!(run(true), (1, 1), "the tag is replaced by the picture; the plain text stays a text paste");
    assert_eq!(run(false), (0, 2), "no picture on the clipboard: both stay text");
}
