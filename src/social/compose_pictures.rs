//! The ways a picture gets into the compose box besides its typed path (SOC-058): the file dialog
//! ("Choose pictures…"), files dropped on the box and a picture pasted from the clipboard. All of
//! them end in `Compose::attach_files` / `attach_pasted`, which apply the checks and the limit of
//! SOC-053. The dialog runs in a background job (it never blocks a frame); tests give the state a
//! pretend `Picker`.

use super::state::{run, Msg, SocialState};
use eframe::egui::{self, Context, Pos2, Rect};
use std::path::PathBuf;
use std::sync::Arc;

/// What the dialog is given: the picture types a post may have (SOC-053).
pub const PICTURE_EXTENSIONS: [&str; 6] = ["jpg", "jpeg", "png", "gif", "webp", "jfif"];

/// A file dialog. `Ok(empty)` is "nothing chosen"; an error is a dialog that could not be shown.
pub trait Picker: Send + Sync {
    /// Ask for up to `max` picture files (it blocks until the person answered: call it from a job).
    fn pick(&self, max: usize) -> Result<Vec<PathBuf>, String>;
}

/// The system's own dialog: the xdg-desktop-portal on Linux, the native one on macOS and Windows.
pub struct SystemPicker;

impl Picker for SystemPicker {
    fn pick(&self, max: usize) -> Result<Vec<PathBuf>, String> {
        let started = std::time::Instant::now();
        let chosen = rfd::FileDialog::new().set_title("Choose pictures or a video").add_filter("Pictures and videos", &[&PICTURE_EXTENSIONS[..], &super::video::VIDEO_EXTENSIONS[..]].concat()).pick_files();
        match chosen {
            Some(mut files) => {
                files.truncate(max);
                Ok(files)
            }
            // rfd answers "nothing" both for Cancel and for a dialog that could not open; nobody
            // cancels within a quarter of a second, so that is a dialog that did not open
            None if cfg!(target_os = "linux") && started.elapsed() < std::time::Duration::from_millis(250) => Err("No file dialog could be opened here (it needs the xdg-desktop-portal). Type the path of the picture instead.".into()),
            None => Ok(vec![]),
        }
    }
}

/// The picture intake of the compose box: the dialog, and the drop the box is waiting for.
pub struct Intake {
    pub picker: Arc<dyn Picker>,
    /// the dialog is open (a second one is not started)
    pub picking: bool,
    /// the dialog's answer, which the box takes when it is drawn
    pub arrived: Option<Result<Vec<PathBuf>, String>>,
    /// where the box was drawn this frame (the app asks before it imports a drop)
    pub box_rect: Option<Rect>,
    /// files dropped from another program on Wayland, which the box takes next frame
    pub queued: Vec<PathBuf>,
    /// something from another program is dragged over the window at this place (Wayland)
    pub ext_hover: Option<Pos2>,
    /// the box took the files dropped in this frame: they are not imported
    pub took_drop: bool,
}

impl Default for Intake {
    fn default() -> Self {
        Intake { picker: Arc::new(SystemPicker), picking: false, arrived: None, box_rect: None, queued: vec![], ext_hover: None, took_drop: false }
    }
}

impl Intake {
    /// Whether a point is over the compose box (the box of this frame).
    pub fn over(&self, p: Pos2) -> bool {
        self.box_rect.is_some_and(|r| r.contains(p))
    }
}

impl SocialState {
    /// Open the file dialog for the compose box in a job; the paths come back as a `Msg::Picked`.
    pub(super) fn pick_pictures(&mut self, ctx: &Context, max: usize) {
        if self.intake.picking {
            return;
        }
        self.intake.picking = true;
        let picker = self.intake.picker.clone();
        run(ctx, &self.tx, move || Msg::Picked { result: picker.pick(max) }, |m| Msg::Picked { result: Err(format!("The file dialog stopped ({m}).")) });
    }
}

/// The pictures pasted into the window this frame, taken out of the events so nothing else pastes
/// them: a clipboard picture, or a pasted text that is only an `<img>` tag while the clipboard
/// holds a picture (a picture copied in a browser).
pub fn take_pasted(ctx: &egui::Context, clipboard_image: impl Fn() -> Option<egui::ColorImage>) -> Vec<Arc<egui::ColorImage>> {
    let mut out = vec![];
    ctx.input_mut(|i| {
        i.events.retain(|e| match e {
            egui::Event::PasteImage(img) => {
                out.push(img.clone());
                false
            }
            egui::Event::Paste(t) if crate::media::is_lone_img_tag(t) => match clipboard_image() {
                Some(img) => {
                    out.push(Arc::new(img));
                    false
                }
                None => true,
            },
            _ => true,
        });
    });
    out
}

/// The files of this frame's drop (window system), as paths.
pub fn dropped_now(ctx: &egui::Context) -> Vec<PathBuf> {
    ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect())
}
