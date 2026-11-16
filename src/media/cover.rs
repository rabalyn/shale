//! Cover art and the quick look at an audio file (MEDIA-003): the pictures that the tags carry
//! (ID3 APIC, FLAC PICTURE, Vorbis METADATA_BLOCK_PICTURE, MP4 `covr`) are read by `symphonia`
//! when it opens the file, the front cover comes first, and the picture is decoded with the
//! `image` crate, scaled down to at most 2048 px. `look` does all of that off the window's thread.

use eframe::egui;
use std::path::Path;
use symphonia::core::meta::{StandardVisualKey, Visual};

/// What the player shows before and while a file plays.
#[derive(Clone)]
pub struct Look {
    /// Length in seconds, when the file says.
    pub duration: Option<f64>,
    /// The cover, if the tags have one that can be read.
    pub cover: Option<egui::ColorImage>,
    /// Why the file cannot be played (a sentence for the player).
    pub error: Option<String>,
}

/// The picture to show: the front cover, else the first picture that is not a back cover or logo.
pub fn pick(visuals: &[Visual]) -> Option<&Visual> {
    let usable = |v: &&Visual| !v.data.is_empty();
    visuals.iter().filter(usable).find(|v| v.usage == Some(StandardVisualKey::FrontCover)).or_else(|| {
        visuals.iter().filter(usable).find(|v| !matches!(v.usage, Some(StandardVisualKey::BackCover | StandardVisualKey::BandArtistLogo | StandardVisualKey::PublisherStudioLogo | StandardVisualKey::FileIcon | StandardVisualKey::OtherIcon)))
    })
}

/// A picture file's bytes as an image of at most 2048 px.
pub fn decode(bytes: &[u8]) -> Option<egui::ColorImage> {
    let img = image::load_from_memory(bytes).ok()?;
    let img = if img.width() > 2048 || img.height() > 2048 { img.thumbnail(2048, 2048) } else { img };
    let rgba = img.to_rgba8();
    Some(egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw()))
}

/// Open the file and read its length and cover. Slow for big files: not for the window's thread.
pub fn look(path: &Path) -> Look {
    match super::decode::open(path) {
        Ok(src) => Look { duration: src.duration, cover: pick(&src.visuals).and_then(|v| decode(&v.data)), error: None },
        Err(e) => {
            // a sound that cannot be played may still have a cover and a length (Opus)
            Look { duration: None, cover: None, error: Some(e) }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::testdata::{fixture, FORMATS};

    // spec: MEDIA-003
    #[test]
    fn the_cover_of_each_kind_of_tag_is_found() {
        for f in FORMATS.iter().filter(|f| f.playable && f.cover) {
            let look = look(&fixture(&format!("tone-cover.{}", f.ext)));
            assert_eq!(look.error, None, "{}", f.ext);
            let cover = look.cover.unwrap_or_else(|| panic!("a cover in the {} file ({})", f.ext, f.tag));
            assert_eq!(cover.size, [64, 48], "{}: the cover is the picture that was embedded", f.ext);
            // the fixture cover is red on the left and blue on the right
            let (left, right) = (cover.pixels[24 * 64 + 5], cover.pixels[24 * 64 + 58]);
            assert!(left.r() > 200 && left.b() < 60, "{}: left {left:?}", f.ext);
            assert!(right.b() > 200 && right.r() < 60, "{}: right {right:?}", f.ext);
        }
    }

    // spec: MEDIA-003
    #[test]
    fn a_file_without_a_cover_has_none() {
        for f in FORMATS.iter().filter(|f| f.playable) {
            let look = look(&fixture(&format!("tone.{}", f.ext)));
            assert_eq!(look.error, None, "{}", f.ext);
            assert!(look.cover.is_none(), "{} has no cover", f.ext);
        }
    }

    // spec: MEDIA-003
    #[test]
    fn the_front_cover_wins_over_other_pictures() {
        let v = |usage, byte: u8| Visual { media_type: None, dimensions: None, color_mode: None, usage, tags: vec![], data: vec![byte].into() };
        let all = [v(Some(StandardVisualKey::BackCover), 1), v(Some(StandardVisualKey::FrontCover), 2), v(None, 3)];
        assert_eq!(pick(&all).unwrap().data[0], 2);
        assert!(pick(&all[..1]).is_none(), "a back cover alone is not the cover");
        assert_eq!(pick(&[all[0].clone(), all[2].clone()]).unwrap().data[0], 3, "a picture without a kind is better than none");
    }

    // spec: MEDIA-003
    #[test]
    fn big_covers_are_scaled_to_2048_and_junk_is_no_cover() {
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(3000, 1500, image::Rgba([10, 20, 30, 255])).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let c = decode(&png).unwrap();
        assert_eq!(c.size, [2048, 1024]);
        assert!(decode(b"not a picture").is_none());
    }
}
