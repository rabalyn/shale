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

/// The `file://` address of a file, as a file manager or Obsidian writes it (spaces as `%20`, the
/// Windows drive after three slashes).
pub fn file_address(path: &std::path::Path) -> String {
    let p = path.to_string_lossy().replace('\\', "/").replace(' ', "%20");
    if p.starts_with('/') {
        format!("file://{p}")
    } else {
        format!("file:///{p}")
    }
}

/// G.711 A-law of a 16-bit sample.
fn alaw(sample: i16) -> u8 {
    let (v, mask) = if sample >= 0 { (sample as i32, 0xD5u8) } else { (-(sample as i32) - 1, 0x55u8) };
    let v = v.min(0x7FFF);
    let (exp, mant) = if v < 256 {
        (0u8, (v >> 4) as u8)
    } else {
        let exp = (32 - (v as u32).leading_zeros() as u8) - 8;
        (exp, ((v >> (exp + 3)) & 0x0F) as u8)
    };
    ((exp << 4) | mant) ^ mask
}

/// G.711 µ-law of a 16-bit sample.
fn mulaw(sample: i16) -> u8 {
    let (v, sign) = if sample < 0 { (-(sample as i32), 0x80u8) } else { (sample as i32, 0) };
    let v = (v.min(32_635) + 0x84) as u32;
    let exp = 31 - v.leading_zeros() as u8 - 7;
    let mant = ((v >> (exp + 3)) & 0x0F) as u8;
    !(sign | (exp << 4) | mant)
}

/// A WAV file of a tone of 440 Hz at 8 kHz, mono, 8 bits: A-law (tag 6) or µ-law (tag 7) bytes,
/// with a `fmt ` chunk of `fmt_len` bytes (16: the form of phone systems such as the FRITZ!Box,
/// which symphonia refuses; 18: with `cbSize`). With tag 0x11 (IMA ADPCM, `fmt_len` 20) the data
/// is not sound but the header is that of such a file, which Shale cannot play.
pub fn wav_bytes(tag: u16, fmt_len: usize, secs: f64) -> Vec<u8> {
    let rate = 8000u32;
    let frames = (rate as f64 * secs) as usize;
    let data: Vec<u8> = (0..frames)
        .map(|i| {
            let s = ((i as f64 * 440.0 * std::f64::consts::TAU / rate as f64).sin() * 0.25 * 32_767.0) as i16;
            match tag {
                6 => alaw(s),
                7 => mulaw(s),
                _ => 0,
            }
        })
        .collect();
    let bits: u16 = if tag == 0x11 { 4 } else { 8 };
    let mut fmt = Vec::new();
    for v in [tag, 1] {
        fmt.extend_from_slice(&v.to_le_bytes());
    }
    for v in [rate, rate] {
        fmt.extend_from_slice(&v.to_le_bytes());
    }
    for v in [1u16, bits] {
        fmt.extend_from_slice(&v.to_le_bytes());
    }
    fmt.resize(fmt_len, 0);
    if tag == 0x11 && fmt_len == 20 {
        // block of 256 bytes, cbSize 2, 505 samples per block
        fmt[12..14].copy_from_slice(&256u16.to_le_bytes());
        fmt[16..18].copy_from_slice(&2u16.to_le_bytes());
        fmt[18..20].copy_from_slice(&505u16.to_le_bytes());
    }
    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&((4 + 8 + fmt.len() + 8 + data.len()) as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&(fmt.len() as u32).to_le_bytes());
    out.extend_from_slice(&fmt);
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&data);
    out
}

/// A file with `bytes` in a folder of its own in the system's temp folder.
pub fn temp_file(name: &str, bytes: &[u8]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("shale-wav-{}-{}", std::process::id(), name.replace('.', "-")));
    std::fs::create_dir_all(&dir).expect("temp folder");
    let path = dir.join(name);
    crate::fsutil::write_atomic(&path, bytes).expect("temp file");
    path
}
