//! Tiny fonts for the tests of the font resolver (ADR-0040): an sfnt file with only the tables
//! that the resolver reads, `name` (family and PostScript name) and `OS/2` (weight, slant,
//! `fsType` and its version), built here from bytes so that no font file with a licence is
//! needed. The files have no outlines and are read, never drawn.
#![cfg(test)]

use std::path::{Path, PathBuf};

/// A test font: its family, weight, slant and licence field.
#[derive(Clone)]
pub struct Spec {
    pub family: &'static str,
    pub weight: u16,
    pub italic: bool,
    pub fs_type: u16,
    /// the version of the OS/2 table; none: the font has no OS/2 table
    pub os2_version: Option<u16>,
}

impl Spec {
    pub fn new(family: &'static str, weight: u16, italic: bool) -> Spec {
        Spec { family, weight, italic, fs_type: 0, os2_version: Some(4) }
    }

    pub fn licence(mut self, fs_type: u16, os2_version: u16) -> Spec {
        self.fs_type = fs_type;
        self.os2_version = Some(os2_version);
        self
    }

    pub fn without_os2(mut self) -> Spec {
        self.os2_version = None;
        self
    }

    /// The bytes of the font.
    pub fn bytes(&self) -> Vec<u8> {
        let utf16 = |s: &str| s.encode_utf16().flat_map(u16::to_be_bytes).collect::<Vec<u8>>();
        let ps = format!("{}-{}{}", self.family.replace(' ', ""), self.weight, if self.italic { "It" } else { "" });
        let strings = [(1u16, utf16(self.family)), (6, utf16(&ps))];
        let mut name = vec![0, 0, 0, strings.len() as u8, 0, (6 + 12 * strings.len()) as u8];
        let mut offset = 0u16;
        for (id, s) in &strings {
            for v in [3u16, 1, 0x409, *id, s.len() as u16, offset] {
                name.extend(v.to_be_bytes());
            }
            offset += s.len() as u16;
        }
        for (_, s) in &strings {
            name.extend(s);
        }
        let mut tables: Vec<(&[u8; 4], Vec<u8>)> = Vec::new();
        if let Some(version) = self.os2_version {
            let len = match version {
                0 => 78,
                1 => 86,
                _ => 96,
            };
            let mut os2 = vec![0u8; len];
            os2[0..2].copy_from_slice(&version.to_be_bytes());
            os2[4..6].copy_from_slice(&self.weight.to_be_bytes());
            os2[6..8].copy_from_slice(&5u16.to_be_bytes());
            os2[8..10].copy_from_slice(&self.fs_type.to_be_bytes());
            let selection: u16 = if self.italic { 1 } else { 0 };
            os2[62..64].copy_from_slice(&selection.to_be_bytes());
            tables.push((b"OS/2", os2));
        }
        tables.push((b"name", name));
        let mut out = vec![0, 1, 0, 0];
        out.extend((tables.len() as u16).to_be_bytes());
        out.extend([0u8; 6]);
        let mut at = 12 + 16 * tables.len();
        for (tag, data) in &tables {
            out.extend(*tag);
            out.extend([0u8; 4]);
            out.extend((at as u32).to_be_bytes());
            out.extend((data.len() as u32).to_be_bytes());
            at += data.len().div_ceil(4) * 4;
        }
        for (_, data) in &tables {
            out.extend(data);
            out.resize(out.len().div_ceil(4) * 4, 0);
        }
        out
    }

    /// Write the font as `dir/file` and return the path.
    pub fn write(&self, dir: &Path, file: &str) -> PathBuf {
        let path = dir.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, self.bytes()).unwrap();
        path
    }
}

/// An empty folder of its own for a test.
pub fn folder(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("shale-fonts-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}
