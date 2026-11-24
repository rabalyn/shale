//! The licence of a font file for embedding: the OS/2 `fsType` field (PDF-038, ADR-0040 §6),
//! read as the OpenType specification says (OS/2 table, `fsType`).

use skrifa::raw::TableProvider;
use skrifa::FontRef;
use std::fmt;

/// What a font's `fsType` allows Shale to do with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Licence {
    /// The font may be embedded (installable, preview and print, editable, or no OS/2 table).
    Free,
    /// The font may be embedded but not subsetted (bit 8): it goes in whole.
    Whole,
    /// The font must not be used.
    Refused(Refusal),
}

/// Why a font may not be embedded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Restricted license embedding (`fsType` 2).
    Restricted,
    /// Bitmap embedding only (bit 9): Shale embeds outlines.
    BitmapOnly,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Refusal::Restricted => "its licence does not allow embedding it",
            Refusal::BitmapOnly => "its licence allows only bitmaps to be embedded",
        })
    }
}

const RESTRICTED: u16 = 0x0002;
const PREVIEW_PRINT: u16 = 0x0004;
const EDITABLE: u16 = 0x0008;
const NO_SUBSETTING: u16 = 0x0100;
const BITMAP_ONLY: u16 = 0x0200;

/// The licence for an `fsType` of a table of the given OS/2 `version`. Bit 0 and the bits 4 to 7
/// and 10 to 15 are reserved and ignored; bits 4 to 15 are ignored in a table of version 0 or 1,
/// as the specification says. Of bits 1 to 3 the least restrictive applies (the specification
/// says so for versions 0 to 2 and that some applications do it for later ones), so a font is
/// restricted only when bit 1 is the only one of the three that is set.
pub fn of_fs_type(fs_type: u16, version: u16) -> Licence {
    if fs_type & (RESTRICTED | PREVIEW_PRINT | EDITABLE) == RESTRICTED {
        return Licence::Refused(Refusal::Restricted);
    }
    if version >= 2 {
        if fs_type & BITMAP_ONLY != 0 {
            return Licence::Refused(Refusal::BitmapOnly);
        }
        if fs_type & NO_SUBSETTING != 0 {
            return Licence::Whole;
        }
    }
    Licence::Free
}

/// The licence of a font: a font without an OS/2 table counts as installable.
pub fn of_font(font: &FontRef) -> Licence {
    font.os2().map(|os2| of_fs_type(os2.fs_type(), os2.version())).unwrap_or(Licence::Free)
}
