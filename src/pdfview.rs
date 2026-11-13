//! Showing PDF files inside notes: pages are rendered with the pure-Rust `hayro` renderer on a
//! background thread and kept as textures (re-rendered when the file changes).

use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

/// Pages rendered to plain images (sRGB, opaque white background).
pub struct Rendered {
    pub images: Vec<ColorImage>,
    /// Page count of the whole document (may be more than rendered).
    pub total: usize,
}

/// Render up to `max_pages` pages of a PDF at `scale` (1.0 = 72 dpi).
pub fn render_pages(data: Vec<u8>, max_pages: usize, scale: f32) -> Result<Rendered, String> {
    use hayro::hayro_syntax::Pdf;
    let pdf = Pdf::new(data).map_err(|e| format!("not a readable PDF ({e:?})"))?;
    let cache = hayro::RenderCache::new();
    let pages = pdf.pages();
    let total = pages.len();
    let mut images = Vec::new();
    for page in pages.iter().take(max_pages) {
        let pm = hayro::render(
            page,
            &cache,
            &Default::default(),
            &Default::default(),
            &hayro::PixmapSettings { x_scale: scale, y_scale: scale, bg_color: hayro::vello_cpu::color::palette::css::WHITE },
        );
        let size = [pm.width() as usize, pm.height() as usize];
        images.push(ColorImage::from_rgba_premultiplied(size, pm.data_as_u8_slice()));
    }
    Ok(Rendered { images, total })
}

pub enum PdfState {
    Loading,
    Ready { pages: Vec<TextureHandle>, total: usize },
    Failed(String),
}

enum Job {
    Running,
    Done(Result<Rendered, String>),
}

type Key = (PathBuf, Option<SystemTime>, usize);

fn jobs() -> &'static Mutex<HashMap<Key, Arc<Mutex<Job>>>> {
    static JOBS: OnceLock<Mutex<HashMap<Key, Arc<Mutex<Job>>>>> = OnceLock::new();
    JOBS.get_or_init(Default::default)
}

/// The first `max_pages` pages of a PDF as textures. Starts rendering in the background on
/// the first call and returns `Loading` until it is done (the UI is asked to repaint then).
pub fn pdf_pages(ctx: &egui::Context, abs: &Path, max_pages: usize) -> PdfState {
    let mtime = std::fs::metadata(abs).and_then(|m| m.modified()).ok();
    let key: Key = (abs.to_path_buf(), mtime, max_pages);
    let tex_id = egui::Id::unique(("shale-pdf", key.clone()));
    if let Some(done) = ctx.data(|d| d.get_temp::<Arc<Result<(Vec<TextureHandle>, usize), String>>>(tex_id)) {
        return match &*done {
            Ok((pages, total)) => PdfState::Ready { pages: pages.clone(), total: *total },
            Err(e) => PdfState::Failed(e.clone()),
        };
    }
    let job = {
        let mut map = jobs().lock().unwrap();
        if let Some(j) = map.get(&key) {
            j.clone()
        } else {
            let j = Arc::new(Mutex::new(Job::Running));
            map.insert(key.clone(), j.clone());
            let (path, jc, ctx2) = (abs.to_path_buf(), j.clone(), ctx.clone());
            std::thread::spawn(move || {
                let result = std::fs::read(&path).map_err(|e| e.to_string()).and_then(|d| render_pages(d, max_pages, 1.5));
                *jc.lock().unwrap() = Job::Done(result);
                ctx2.request_repaint();
            });
            j
        }
    };
    let mut guard = job.lock().unwrap();
    match std::mem::replace(&mut *guard, Job::Running) {
        Job::Running => PdfState::Loading,
        Job::Done(result) => {
            let stored: Result<(Vec<TextureHandle>, usize), String> = result.map(|r| {
                let pages = r.images.into_iter().enumerate().map(|(i, img)| ctx.load_texture(format!("{}#{i}", abs.display()), img, TextureOptions::LINEAR)).collect();
                (pages, r.total)
            });
            ctx.data_mut(|d| d.insert_temp(tex_id, Arc::new(stored.clone_ok())));
            jobs().lock().unwrap().remove(&key);
            match stored {
                Ok((pages, total)) => PdfState::Ready { pages, total },
                Err(e) => PdfState::Failed(e),
            }
        }
    }
}

trait CloneOk {
    fn clone_ok(&self) -> Self;
}
impl CloneOk for Result<(Vec<TextureHandle>, usize), String> {
    fn clone_ok(&self) -> Self {
        match self {
            Ok((p, t)) => Ok((p.clone(), *t)),
            Err(e) => Err(e.clone()),
        }
    }
}

pub fn is_pdf(p: &Path) -> bool {
    p.extension().map(|e| e.eq_ignore_ascii_case("pdf")).unwrap_or(false)
}

#[cfg(test)]
pub(crate) fn sample_pdf() -> Vec<u8> {
    let mut d = crate::pdfwrite::Doc::new("sample");
    for i in 0..3 {
        let p = d.add_page(300.0, 200.0);
        d.page(p).rect(20.0, 20.0, 260.0, 160.0, Some(crate::pdfwrite::Rgba(40, 90, 200, 255)), None);
        d.page(p).text(crate::pdfwrite::FontKind::Sans, 24.0, 40.0, 110.0, &format!("Page {}", i + 1), crate::pdfwrite::Rgba(255, 255, 255, 255), false, false);
    }
    d.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_pages() {
        let r = render_pages(sample_pdf(), 2, 1.0).unwrap();
        assert_eq!(r.total, 3);
        assert_eq!(r.images.len(), 2);
        assert_eq!(r.images[0].size, [300, 200]);
        assert!(render_pages(b"not a pdf".to_vec(), 1, 1.0).is_err());
    }

    #[test]
    fn background_rendering_becomes_ready() {
        let dir = std::env::temp_dir().join(format!("shale-pdfview-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("s.pdf");
        std::fs::write(&f, sample_pdf()).unwrap();
        let ctx = egui::Context::default();
        let mut state = pdf_pages(&ctx, &f, 5);
        for _ in 0..200 {
            if !matches!(state, PdfState::Loading) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
            state = pdf_pages(&ctx, &f, 5);
        }
        match state {
            PdfState::Ready { pages, total } => {
                assert_eq!(total, 3);
                assert_eq!(pages.len(), 3);
            }
            _ => panic!("PDF should render"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
