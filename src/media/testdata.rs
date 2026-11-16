//! The small sound files of the media tests (made by `scripts/make-media-fixtures.sh`, MEDIA-011).

use std::path::PathBuf;

pub struct Format {
    pub ext: &'static str,
    /// the kind of tag that carries the cover in this format
    pub tag: &'static str,
    /// Shale can decode it (symphonia has no Opus decoder)
    pub playable: bool,
    /// a variant with a cover exists (`tone-cover.<ext>`)
    pub cover: bool,
}

pub const FORMATS: [Format; 8] = [
    Format { ext: "mp3", tag: "ID3 APIC", playable: true, cover: true },
    Format { ext: "ogg", tag: "Vorbis METADATA_BLOCK_PICTURE", playable: true, cover: true },
    Format { ext: "oga", tag: "Vorbis METADATA_BLOCK_PICTURE", playable: true, cover: true },
    Format { ext: "opus", tag: "Vorbis METADATA_BLOCK_PICTURE", playable: false, cover: true },
    Format { ext: "flac", tag: "FLAC PICTURE", playable: true, cover: true },
    Format { ext: "m4a", tag: "MP4 covr", playable: true, cover: true },
    Format { ext: "aac", tag: "none (ADTS)", playable: true, cover: false },
    Format { ext: "wav", tag: "none", playable: true, cover: false },
];

/// A file of `src/testdata/media/`.
pub fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/testdata/media").join(name)
}
