//! Cutting a TrueType font down to the glyphs of a document (PDF-036, ADR-0040 §4).

use super::sfnt::{build, has_glyf, subset, tables, whole};
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{FontRef, MetadataProvider};
use std::collections::BTreeSet;

const BASE: &[u8] = include_bytes!("../../assets/fonts/LiberationSerif-Regular.ttf");

/// The number of path commands of a glyph; 0 for an empty glyph.
#[derive(Default)]
struct Count(usize);

impl OutlinePen for Count {
    fn move_to(&mut self, _: f32, _: f32) {
        self.0 += 1;
    }
    fn line_to(&mut self, _: f32, _: f32) {
        self.0 += 1;
    }
    fn quad_to(&mut self, _: f32, _: f32, _: f32, _: f32) {
        self.0 += 1;
    }
    fn curve_to(&mut self, _: f32, _: f32, _: f32, _: f32, _: f32, _: f32) {
        self.0 += 1;
    }
    fn close(&mut self) {
        self.0 += 1;
    }
}

fn commands(font: &FontRef, c: char) -> usize {
    let g = font.charmap().map(c).expect("the font has the character");
    let mut count = Count::default();
    if let Some(outline) = font.outline_glyphs().get(g) {
        outline.draw(DrawSettings::unhinted(Size::unscaled(), LocationRef::default()), &mut count).unwrap();
    }
    count.0
}

fn gid(c: char) -> u16 {
    FontRef::new(BASE).unwrap().charmap().map(c).unwrap().to_u32() as u16
}

// spec: PDF-036
#[test]
fn a_subset_keeps_the_outlines_of_the_glyphs_it_is_asked_for_and_their_components_and_empties_the_others() {
    let used: BTreeSet<u16> = ['A', 'é'].map(gid).into();
    let cut = subset(BASE, 0, &used).expect("a TrueType font is cut down");
    assert!(cut.len() < BASE.len() / 4, "{} of {} bytes", cut.len(), BASE.len());
    let (orig, small) = (FontRef::new(BASE).unwrap(), FontRef::new(&cut).unwrap());
    assert_eq!(commands(&small, 'A'), commands(&orig, 'A'));
    assert!(commands(&small, 'A') > 4);
    assert_eq!(commands(&small, 'B'), 0, "a glyph that is not used is empty");
    // é is a composite of e and an accent: its parts are kept, so it is drawn whole
    assert_eq!(commands(&small, 'é'), commands(&orig, 'é'));
    assert!(commands(&small, 'é') > 0);
    // the character map and the widths are as they were
    assert_eq!(small.charmap().map('B'), orig.charmap().map('B'));
    let width = |f: &FontRef, c: char| f.glyph_metrics(Size::unscaled(), LocationRef::default()).advance_width(f.charmap().map(c).unwrap());
    assert_eq!(width(&small, 'B'), width(&orig, 'B'));
}

// spec: PDF-036
#[test]
fn a_face_of_a_collection_is_taken_out_whole_and_a_font_without_outlines_or_a_wrong_index_is_none() {
    let font = tables(BASE, 0).unwrap();
    // a collection of this one font: its table records point into the file
    let single = build(font.iter().map(|(t, d)| (*t, d.to_vec())).collect());
    let mut ttc = b"ttcf\0\x01\0\0\0\0\0\x01\0\0\0\x10".to_vec();
    ttc.extend(&single);
    for n in 0..font.len() {
        let rec = 16 + 12 + 16 * n + 8;
        let offset = u32::from_be_bytes(ttc[rec..rec + 4].try_into().unwrap()) + 16;
        ttc[rec..rec + 4].copy_from_slice(&offset.to_be_bytes());
    }
    assert_eq!(tables(&ttc, 0).unwrap().len(), font.len());
    assert!(tables(&ttc, 1).is_none() && tables(BASE, 1).is_none());
    let out = whole(&ttc, 0).unwrap();
    assert!(FontRef::new(&out).is_ok(), "the face is a font of its own");
    assert_eq!(commands(&FontRef::new(&out).unwrap(), 'A'), commands(&FontRef::new(BASE).unwrap(), 'A'));
    assert_eq!(whole(BASE, 0).unwrap(), BASE);
    let cut = subset(&ttc, 0, &BTreeSet::from([gid('A')])).unwrap();
    assert_eq!(commands(&FontRef::new(&cut).unwrap(), 'A'), commands(&FontRef::new(BASE).unwrap(), 'A'));
    // without a glyf table there is nothing to cut, and the font is drawn as outlines
    let cff = include_bytes!("../../assets/fonts/Virgil.otf");
    assert!(has_glyf(BASE, 0) && !has_glyf(cff, 0));
    assert!(subset(cff, 0, &BTreeSet::from([1])).is_none());
    assert!(tables(b"ttcf", 0).is_none() && tables(&BASE[..40], 0).is_none());
}
