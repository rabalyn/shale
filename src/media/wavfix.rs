//! A WAV file with an A-law or µ-law sound and a 16-byte `fmt ` chunk (MEDIA-025, #581).
//!
//! The format tags 6 (A-law) and 7 (µ-law) are written by phone systems, answering machines (a
//! FRITZ!Box among them) and many recorders with the plain 16-byte `fmt ` chunk, without the
//! `cbSize` field of the extended chunk. `symphonia-format-riff` 0.6.1 accepts only the 18-byte
//! form (`read_alaw_pcm_fmt` and `read_mulaw_pcm_fmt` in `src/wave/chunks.rs` fail with "malformed
//! fmt_alaw chunk" otherwise), although VLC, ffmpeg and every other player read the short one. The
//! patch for the crate is `docs/upstream/symphonia-riff-alaw-fmt16.patch`; until it is released
//! and used, [`Widened`] shows symphonia the file as if the chunk had `cbSize = 0`: the bytes up to
//! the end of the chunk come from a patched copy in memory (the chunk's size 18, the RIFF size
//! two bytes more), then two zero bytes, then the rest of the file as it is. Every other file
//! passes through unchanged.

use std::io::{self, Read, Seek, SeekFrom};
use symphonia::core::io::MediaSource;

const WAVE_FORMAT_ALAW: u16 = 6;
const WAVE_FORMAT_MULAW: u16 = 7;
/// The chunks looked through before giving up on finding `fmt ` (a few `JUNK`/`bext` ones come first).
const MAX_CHUNKS: usize = 32;
/// The bytes `cbSize` adds.
const CB_SIZE: u64 = 2;

/// A file as symphonia is shown it: the file itself, or the widened form described above.
pub struct Widened<R> {
    inner: R,
    /// The patched start of the file, up to the end of the `fmt ` chunk (empty: nothing is changed).
    head: Vec<u8>,
    /// Zero bytes inserted behind `head` (0 or 2).
    extra: u64,
    len: u64,
    pos: u64,
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// Where the `fmt ` chunk of a WAV file ends and what it says: `(end of its body, its size, format tag)`.
/// None when `r` is no WAV file or has no (readable) `fmt ` chunk before its `data`.
fn fmt_chunk<R: Read + Seek>(r: &mut R) -> io::Result<Option<(u64, u64, u16)>> {
    r.seek(SeekFrom::Start(0))?;
    let mut riff = [0u8; 12];
    if r.read_exact(&mut riff).is_err() || &riff[..4] != b"RIFF" || &riff[8..] != b"WAVE" {
        return Ok(None);
    }
    let mut at = 12u64;
    for _ in 0..MAX_CHUNKS {
        let mut h = [0u8; 8];
        if r.read_exact(&mut h).is_err() {
            return Ok(None);
        }
        let size = u32_at(&h, 4) as u64;
        match &h[..4] {
            b"fmt " => {
                let mut tag = [0u8; 2];
                return Ok((size >= 2 && r.read_exact(&mut tag).is_ok()).then(|| (at + 8 + size, size, u16::from_le_bytes(tag))));
            }
            b"data" => return Ok(None),
            _ => {}
        }
        at += 8 + size + (size & 1);
        r.seek(SeekFrom::Start(at))?;
    }
    Ok(None)
}

/// The format tag of a WAV file's `fmt ` chunk (1 PCM, 2 and 0x11 ADPCM, 6 A-law, 7 µ-law, …),
/// None for anything else.
pub fn wav_format<R: Read + Seek>(r: &mut R) -> Option<u16> {
    fmt_chunk(r).ok().flatten().map(|(_, _, tag)| tag)
}

/// The end of the 16-byte `fmt ` chunk of an A-law or µ-law WAV file, if that is what `r` is.
fn short_fmt_end<R: Read + Seek>(r: &mut R) -> io::Result<Option<u64>> {
    Ok(fmt_chunk(r)?.filter(|(_, size, tag)| *size == 16 && matches!(*tag, WAVE_FORMAT_ALAW | WAVE_FORMAT_MULAW)).map(|(end, ..)| end))
}

impl<R: Read + Seek> Widened<R> {
    pub fn new(mut inner: R) -> io::Result<Widened<R>> {
        let len = inner.seek(SeekFrom::End(0))?;
        let (mut head, mut extra) = (Vec::new(), 0);
        if let Some(end) = short_fmt_end(&mut inner)?.filter(|e| *e <= len) {
            inner.seek(SeekFrom::Start(0))?;
            head = vec![0u8; end as usize];
            inner.read_exact(&mut head)?;
            // the RIFF size grows by the two bytes, and the chunk's size is 18
            let riff = u32_at(&head, 4).saturating_add(CB_SIZE as u32);
            head[4..8].copy_from_slice(&riff.to_le_bytes());
            let fmt_size_at = end as usize - 16 - 4;
            head[fmt_size_at..fmt_size_at + 4].copy_from_slice(&18u32.to_le_bytes());
            extra = CB_SIZE;
        }
        inner.seek(SeekFrom::Start(0))?;
        Ok(Widened { inner, head, extra, len: len + extra, pos: 0 })
    }
}

impl<R: Read + Seek> Read for Widened<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let head = self.head.len() as u64;
        if self.pos >= self.len || buf.is_empty() {
            return Ok(0);
        }
        let n = if self.pos < head {
            let n = buf.len().min((head - self.pos) as usize);
            buf[..n].copy_from_slice(&self.head[self.pos as usize..self.pos as usize + n]);
            n
        } else if self.pos < head + self.extra {
            let n = buf.len().min((head + self.extra - self.pos) as usize);
            buf[..n].fill(0);
            n
        } else {
            self.inner.seek(SeekFrom::Start(self.pos - self.extra))?;
            self.inner.read(buf)?
        };
        self.pos += n as u64;
        Ok(n)
    }
}

impl<R: Read + Seek> Seek for Widened<R> {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let new = match to {
            SeekFrom::Start(p) => Some(p),
            SeekFrom::Current(d) => self.pos.checked_add_signed(d),
            SeekFrom::End(d) => self.len.checked_add_signed(d),
        };
        self.pos = new.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "seek before the start of the file"))?;
        Ok(self.pos)
    }
}

impl<R: Read + Seek + Send + Sync> MediaSource for Widened<R> {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        Some(self.len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::testdata::wav_bytes;
    use proptest::prelude::*;
    use std::io::Cursor;

    fn all(bytes: &[u8]) -> Vec<u8> {
        let mut w = Widened::new(Cursor::new(bytes.to_vec())).unwrap();
        let mut out = Vec::new();
        w.read_to_end(&mut out).unwrap();
        assert_eq!(out.len() as u64, w.byte_len().unwrap(), "the length is what reading gives");
        out
    }

    // spec: MEDIA-025
    #[test]
    fn media_025_a_16_byte_alaw_or_mulaw_fmt_chunk_gets_cbsize_and_nothing_else_changes() {
        for tag in [6u16, 7] {
            let file = wav_bytes(tag, 16, 0.1);
            let wide = all(&file);
            assert_eq!(wide.len(), file.len() + 2);
            assert_eq!(&wide[..4], b"RIFF");
            assert_eq!(u32::from_le_bytes(wide[4..8].try_into().unwrap()) as usize, wide.len() - 8, "the RIFF size counts the two bytes");
            assert_eq!(&wide[12..16], b"fmt ");
            assert_eq!(u32::from_le_bytes(wide[16..20].try_into().unwrap()), 18);
            assert_eq!(&wide[20..36], &file[20..36], "the 16 bytes of the chunk");
            assert_eq!(&wide[36..38], &[0, 0], "cbSize is 0");
            assert_eq!(&wide[38..], &file[36..], "data chunk and the rest as they were");
        }
    }

    // spec: MEDIA-025
    #[test]
    fn media_025_other_files_pass_through_unchanged() {
        let alaw18 = wav_bytes(6, 18, 0.1);
        assert_eq!(all(&alaw18), alaw18, "the 18-byte form is fine as it is");
        let adpcm = wav_bytes(0x11, 20, 0.01);
        assert_eq!(all(&adpcm), adpcm);
        let mut pcm16 = wav_bytes(6, 16, 0.01);
        pcm16[20] = 1;
        assert_eq!(all(&pcm16), pcm16, "a 16-byte chunk of plain PCM is fine");
        for junk in [&b""[..], b"RIFF", b"RIFF\0\0\0\0WAVE", b"RIFF\0\0\0\0WAVEfmt ", b"not a sound at all, not a sound at all"] {
            assert_eq!(all(junk), junk);
        }
    }

    // spec: MEDIA-025
    #[test]
    fn media_025_the_chunks_before_fmt_are_skipped_and_reading_and_seeking_work_in_the_middle() {
        let file = wav_bytes(6, 16, 0.1);
        // a JUNK chunk of odd size (with its pad byte) before fmt
        let mut with_junk = file[..12].to_vec();
        with_junk.extend_from_slice(b"JUNK\x03\0\0\0abc\0");
        with_junk.extend_from_slice(&file[12..]);
        let wide = all(&with_junk);
        assert_eq!(wide.len(), with_junk.len() + 2);
        let at = 12 + 12;
        assert_eq!(&wide[at..at + 4], b"fmt ");
        assert_eq!(u32::from_le_bytes(wide[at + 4..at + 8].try_into().unwrap()), 18);
        // seeks into the head, the inserted bytes and the data read the same as a straight read
        let mut w = Widened::new(Cursor::new(with_junk)).unwrap();
        for (from, n) in [(3u64, 10usize), (at as u64 + 8 + 15, 6), (at as u64 + 8 + 16, 2), (wide.len() as u64 - 20, 20)] {
            w.seek(SeekFrom::Start(from)).unwrap();
            let mut buf = vec![0u8; n];
            w.read_exact(&mut buf).unwrap();
            assert_eq!(buf, wide[from as usize..from as usize + n], "from {from}");
        }
        assert_eq!(w.seek(SeekFrom::End(-4)).unwrap(), wide.len() as u64 - 4);
        assert_eq!(w.seek(SeekFrom::Current(2)).unwrap(), wide.len() as u64 - 2);
        assert!(w.seek(SeekFrom::Current(-1_000_000)).is_err(), "before the start");
        assert!(w.is_seekable());
    }

    // spec: MEDIA-025
    #[test]
    fn media_025_the_format_tag_of_a_wav_file_is_read() {
        for (tag, len) in [(6u16, 16usize), (7, 18), (0x11, 20), (1, 16)] {
            assert_eq!(wav_format(&mut Cursor::new(wav_bytes(tag, len, 0.01))), Some(tag));
        }
        assert_eq!(wav_format(&mut Cursor::new(b"RIFFxxxxWAVEdata\0\0\0\0".to_vec())), None, "data before fmt");
        assert_eq!(wav_format(&mut Cursor::new(b"ID3".to_vec())), None);
        // chunks that never end in a fmt chunk are given up on
        let mut many = b"RIFF\0\0\0\0WAVE".to_vec();
        for _ in 0..MAX_CHUNKS + 1 {
            many.extend_from_slice(b"JUNK\0\0\0\0");
        }
        many.extend_from_slice(&wav_bytes(6, 16, 0.01)[12..]);
        assert_eq!(wav_format(&mut Cursor::new(many)), None);
    }

    proptest! {
        // spec: MEDIA-025
        #[test]
        fn media_025_whatever_the_bytes_the_shim_does_not_panic_and_only_a_short_fmt_grows_by_two(
            bytes in proptest::collection::vec(any::<u8>(), 0..200),
            tag in prop::sample::select(vec![1u16, 6, 7, 0x11]),
            fmt_len in prop::sample::select(vec![16usize, 18, 20]),
            samples in 0usize..400,
        ) {
            prop_assert_eq!(all(&bytes).len(), bytes.len());
            let file = wav_bytes(tag, fmt_len, samples as f64 / 8000.0);
            let wide = all(&file);
            let grows = (tag == 6 || tag == 7) && fmt_len == 16;
            prop_assert_eq!(wide.len(), file.len() + if grows { 2 } else { 0 });
            // the sound itself is the same bytes
            prop_assert_eq!(&wide[wide.len() - samples..], &file[file.len() - samples..]);
        }
    }
}
