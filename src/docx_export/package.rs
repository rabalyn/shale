//! A Word file as its parts in memory: read, changed here and there, packed again (DOCX-007, DOCX-009).

use crate::import::docx_look::zip_bytes;
use crate::pdf::tex::archive;

/// What a Word file starts with: the zip is read and written with this part first.
const FIRST: &str = "[Content_Types].xml";

#[derive(Debug)]
pub struct Package {
    files: Vec<(String, Vec<u8>)>,
}

impl Package {
    pub fn read(bytes: &[u8]) -> Result<Package, String> {
        let files = archive::read_zip_files(bytes)?;
        if !files.iter().any(|(n, _)| n == FIRST) {
            return Err("it is no Word file (it has no [Content_Types].xml)".into());
        }
        Ok(Package { files })
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.files.iter().map(|(n, _)| n.as_str())
    }

    /// A part as text.
    pub fn text(&self, name: &str) -> Result<&str, String> {
        let (_, bytes) = self.files.iter().find(|(n, _)| n == name).ok_or_else(|| format!("it has no {name}"))?;
        std::str::from_utf8(bytes).map_err(|_| format!("{name} is not UTF-8 text"))
    }

    /// Replace a part that is there.
    pub fn set(&mut self, name: &str, text: String) {
        if let Some(f) = self.files.iter_mut().find(|(n, _)| n == name) {
            f.1 = text.into_bytes();
        }
    }

    /// Leave the parts out that `keep` says no to.
    pub fn retain(&mut self, mut keep: impl FnMut(&str) -> bool) {
        self.files.retain(|(n, _)| keep(n));
    }

    /// The zip, compressed, `[Content_Types].xml` first.
    pub fn pack(mut self) -> Result<Vec<u8>, String> {
        self.files.sort_by_key(|(n, _)| n != FIRST);
        zip_bytes(&self.files, true)
    }
}
