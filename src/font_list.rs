//! The fonts of the entries of the font picker (CFG-077): each family is drawn in its own font.
//!
//! Only the rows that are on screen are asked for, and only once the list has rested for
//! [`SETTLE`] seconds (a fast scroll loads nothing). Their regular faces are read by a worker
//! thread (font files can be large), kept in a list of at most [`KEPT`] faces (the ones seen
//! last), and handed to egui in one batch, each under a family of its own: one `set_fonts`
//! per rest of the list, never one per row or per frame. Until a face is in egui, the row shows
//! the name in Shale's font (the same row height, so nothing jumps). A face whose letters are
//! pictures (a symbol font) is shown as the name in Shale's font with a sample in the font.

use eframe::egui;
use std::collections::BTreeSet;
use std::sync::{mpsc, Arc};

/// How many faces stay loaded at most.
pub const KEPT: usize = 64;

/// How long (in seconds) the rows on screen must stay the same before faces are read and handed
/// to egui, so scrolling past hundreds of families loads none of them.
pub const SETTLE: f64 = 0.15;

/// The sample that follows the name of a symbol font.
pub const SYMBOL_SAMPLE: &str = "Aa Bb 12";

/// A face that was read.
#[derive(Clone)]
pub struct ListFace {
    pub name: String,
    pub data: Arc<egui::FontData>,
    /// its letters are not the letters of its name (a symbol font)
    pub symbol: bool,
}

/// What the worker answers: the face of a family, `None` when it has none.
struct Reply {
    name: String,
    face: Option<ListFace>,
}

#[derive(Default)]
pub struct ListFonts {
    requests: Option<mpsc::Sender<String>>,
    replies: Option<mpsc::Receiver<Reply>>,
    /// the rows on screen, since when they are the same, and whether this frame named them
    wanted: Vec<String>,
    since: f64,
    touched: bool,
    in_flight: BTreeSet<String>,
    /// families without a face, not asked for again
    none: BTreeSet<String>,
    /// the faces that were read, the one seen last at the end
    loaded: Vec<ListFace>,
    /// the faces that are in egui (or on their way): what `loaded` was at the last batch
    included: Vec<ListFace>,
}

impl ListFonts {
    /// The rows on screen in this frame (by family name).
    pub fn want(&mut self, names: Vec<String>, now: f64) {
        self.touched = true;
        if names != self.wanted {
            self.wanted = names;
            self.since = now;
        }
        let Self { wanted, loaded, .. } = self;
        for name in wanted.iter() {
            if let Some(i) = loaded.iter().position(|l| l.name == *name) {
                let l = loaded.remove(i);
                loaded.push(l);
            }
        }
    }

    /// The faces that go into the fonts of egui.
    pub fn included(&self) -> &[ListFace] {
        &self.included
    }

    /// How many faces are kept (read, maybe not in egui yet).
    #[cfg(test)]
    pub fn kept(&self) -> usize {
        self.loaded.len()
    }

    /// Whether a worker still reads.
    #[cfg(test)]
    pub fn reading(&self) -> bool {
        !self.in_flight.is_empty()
    }

    /// One step per frame: take what the worker has read, ask for the faces of the rows once the
    /// list has rested, and decide when the batch goes to egui. Comes back `true` when
    /// [`ListFonts::included`] changed, which needs a `set_fonts`.
    pub fn step(&mut self, ctx: &egui::Context, db: Option<&Arc<fontdb::Database>>, now: f64) -> bool {
        let touched = std::mem::take(&mut self.touched);
        if !touched {
            // the picker is closed: let go of everything
            self.wanted.clear();
            self.loaded.clear();
            self.in_flight.clear();
            self.replies = None;
            self.requests = None;
            return !std::mem::take(&mut self.included).is_empty();
        }
        let answers: Vec<Reply> = self.replies.iter().flat_map(|rx| rx.try_iter()).collect();
        for r in answers {
            self.in_flight.remove(&r.name);
            match r.face {
                Some(f) if self.wanted.contains(&f.name) => self.keep(f),
                Some(_) => {}
                None => {
                    self.none.insert(r.name);
                }
            }
        }
        let rest = self.since + SETTLE - now;
        if rest > 0.0 {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(rest));
            return false;
        }
        if let Some(db) = db {
            let asks: Vec<String> = self.wanted.iter().filter(|n| !self.has(n)).cloned().collect();
            for name in asks {
                if self.ask(ctx, db, &name) {
                    self.in_flight.insert(name);
                }
            }
        }
        // one batch for the faces of a rest: when the worker has read them all
        if self.in_flight.is_empty() && self.loaded.iter().any(|l| !self.included.iter().any(|i| i.name == l.name)) {
            self.included = self.loaded.clone();
            return true;
        }
        false
    }

    /// Whether the face of `name` is read, being read, or known not to exist.
    fn has(&self, name: &str) -> bool {
        self.loaded.iter().any(|l| l.name == name) || self.in_flight.contains(name) || self.none.contains(name)
    }

    /// Keep a face, dropping the ones seen longest ago (never one of the rows on screen) when
    /// there are more than [`KEPT`].
    fn keep(&mut self, face: ListFace) {
        self.loaded.push(face);
        while self.loaded.len() > KEPT {
            let at = self.loaded.iter().position(|l| !self.wanted.contains(&l.name)).unwrap_or(0);
            self.loaded.remove(at);
        }
    }

    /// Send the request for `name` to the worker (started with the first request).
    fn ask(&mut self, ctx: &egui::Context, db: &Arc<fontdb::Database>, name: &str) -> bool {
        if self.requests.is_none() {
            let (tx, rx) = mpsc::channel::<String>();
            let (rtx, rrx) = mpsc::channel::<Reply>();
            let (db, ctx) = (db.clone(), ctx.clone());
            let spawned = std::thread::Builder::new().name("font-list".into()).spawn(move || {
                while let Ok(name) = rx.recv() {
                    let face = read_face(&db, &name);
                    if rtx.send(Reply { name, face }).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            });
            if spawned.is_err() {
                return false;
            }
            self.requests = Some(tx);
            self.replies = Some(rrx);
        }
        self.requests.as_ref().is_some_and(|tx| tx.send(name.to_string()).is_ok())
    }
}

/// Read the regular face of `name` (what the worker does).
fn read_face(db: &fontdb::Database, name: &str) -> Option<ListFace> {
    let (bytes, index) = crate::fonts::regular_face(db, name)?;
    let symbol = is_symbol(&bytes, index, name);
    let mut data = egui::FontData::from_owned(bytes);
    data.index = index;
    Some(ListFace { name: name.to_string(), data: Arc::new(data), symbol })
}

/// Whether a font cannot show its own name: its character map is a symbol one (the letters are
/// pictures), or it has no glyph for a letter of the name.
pub fn is_symbol(bytes: &[u8], index: u32, name: &str) -> bool {
    use skrifa::MetadataProvider;
    let Ok(font) = skrifa::FontRef::from_index(bytes, index) else { return false };
    let map = font.charmap();
    map.is_symbol() || name.chars().filter(|c| c.is_alphabetic()).any(|c| map.map(c).is_none())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn face(name: &str) -> ListFace {
        ListFace { name: name.into(), data: Arc::new(egui::FontData::from_static(&[])), symbol: false }
    }

    // spec: CFG-077
    #[test]
    fn cfg_077_at_most_kept_faces_stay_and_the_rows_on_screen_are_not_dropped() {
        let mut l = ListFonts::default();
        l.want(vec!["F0".into(), "F1".into()], 0.0);
        for i in 0..(KEPT + 20) {
            l.keep(face(&format!("F{i}")));
            assert!(l.kept() <= KEPT);
        }
        assert_eq!(l.kept(), KEPT);
        let names: Vec<&str> = l.loaded.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"F0") && names.contains(&"F1"), "the rows on screen stay");
        assert!(names.contains(&format!("F{}", KEPT + 19).as_str()) && !names.contains(&"F2"), "the ones seen longest ago go");
        // a row that is seen again moves to the end, so it goes last
        l.want(vec!["F30".into()], 2.0);
        assert_eq!(l.loaded.last().map(|f| f.name.as_str()), Some("F30"));
    }

    // spec: CFG-077
    #[test]
    fn cfg_077_a_symbol_font_is_told_from_one_that_can_show_its_name() {
        let virgil = include_bytes!("../assets/fonts/Virgil.otf");
        assert!(!is_symbol(virgil, 0, "Virgil"), "the letters of the name are there");
        assert!(is_symbol(virgil, 0, "\u{65e5}\u{672c}\u{8a9e} Virgil"), "a name the font has no letters for stays readable only in Shale's font");
        assert!(!is_symbol(b"not a font", 0, "Virgil"), "bytes that are no font are not told apart");
        assert!(!is_symbol(epaint_default_fonts::HACK_REGULAR, 0, "Hack"));
    }
}
