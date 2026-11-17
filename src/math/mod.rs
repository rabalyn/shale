//! Math typesetting in pure Rust (EDIT-046 to EDIT-049, PDF-012): the TeX subset is parsed
//! (`parse`), laid out with the font's MATH table (`layout`, `font`) and drawn on the screen as a
//! sharp mask in the text colour (`raster`, `paint`) or in the PDF as glyphs and rules.

pub mod font;
pub mod layout;
pub mod paint;
pub mod parse;
pub mod raster;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// The size of a display formula in parts of the text around it.
pub const DISPLAY_SCALE: f32 = 1.15;

/// A formula that was typeset, or the reason it could not be (EDIT-049).
pub type Typeset = Result<Arc<Formula>, String>;

/// A typeset formula with a number that tells it from every other one (the screen's pictures of
/// it are kept under that number).
#[derive(Debug)]
pub struct Formula {
    pub id: u64,
    pub layout: layout::Layout,
}

/// What a layout is cached by (EDIT-048): the TeX, the size and the style.
type Key = (String, u32, bool);

/// The most layouts kept; past it the cache starts again (a note rarely has more formulas).
const CACHE_LIMIT: usize = 4096;

/// Typeset `tex` with an em of `size` points, in display style or in text style. The layouts are
/// cached by TeX, size and style, so a note is not laid out again on every frame (EDIT-048).
pub fn typeset(tex: &str, size: f32, display: bool) -> Typeset {
    static CACHE: Mutex<Option<HashMap<Key, Typeset>>> = Mutex::new(None);
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let key = (tex.trim().to_string(), size.to_bits(), display);
    if let Some(hit) = CACHE.lock().ok().and_then(|c| c.as_ref().and_then(|m| m.get(&key).cloned())) {
        return hit;
    }
    let made: Typeset = parse::parse(&key.0).map_err(|e| e.to_string()).and_then(|node| layout::layout(&node, size, display)).map(|layout| Arc::new(Formula { id: NEXT.fetch_add(1, Ordering::Relaxed), layout }));
    if let Ok(mut c) = CACHE.lock() {
        let map = c.get_or_insert_with(HashMap::new);
        if map.len() >= CACHE_LIMIT {
            map.clear();
        }
        map.insert(key, made.clone());
    }
    made
}
