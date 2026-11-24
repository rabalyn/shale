//! The licence of a font file: OS/2 `fsType` (PDF-038, ADR-0040 §6).

use super::licence::{of_font, of_fs_type, Licence, Refusal};
use super::test_fonts::Spec;
use skrifa::FontRef;

fn licence_of(spec: Spec) -> Licence {
    let bytes = spec.bytes();
    of_font(&FontRef::new(&bytes).expect("the test font is a font"))
}

// spec: PDF-038
#[test]
fn pdf_038_each_embedding_bit_is_read_as_the_opentype_specification_says() {
    use Licence::{Free, Refused, Whole};
    let restricted = Refused(Refusal::Restricted);
    let bitmap = Refused(Refusal::BitmapOnly);
    for (fs_type, expected) in [
        (0x0000, Free),       // installable
        (0x0002, restricted), // restricted license embedding
        (0x0004, Free),       // preview and print
        (0x0008, Free),       // editable
        (0x0100, Whole),      // no subsetting: allowed, embedded whole
        (0x0200, bitmap),     // bitmap embedding only
        (0x0300, bitmap),     // both: refused
        (0x0102, restricted),
        (0x0104, Whole),
        (0x0108, Whole),
        (0x0204, bitmap),
    ] {
        assert_eq!(of_fs_type(fs_type, 4), expected, "fsType {fs_type:#06x}");
    }
}

// spec: PDF-038
#[test]
fn pdf_038_of_bits_one_to_three_the_least_restrictive_applies_and_restricted_needs_to_stand_alone() {
    for fs_type in [0x0006, 0x000a, 0x000e] {
        assert_eq!(of_fs_type(fs_type, 2), Licence::Free, "{fs_type:#06x} in a version 2 table");
        assert_eq!(of_fs_type(fs_type, 4), Licence::Free, "{fs_type:#06x} in a version 4 table");
    }
    assert_eq!(of_fs_type(0x0002, 0), Licence::Refused(Refusal::Restricted), "alone, in every version");
}

// spec: PDF-038
#[test]
fn pdf_038_reserved_bits_are_ignored_and_bits_4_to_15_are_ignored_in_a_table_of_version_0_or_1() {
    // bit 0 and the reserved bits 4 to 7 and 10 to 15 do nothing
    assert_eq!(of_fs_type(0x0001, 4), Licence::Free);
    assert_eq!(of_fs_type(0x00f0, 4), Licence::Free);
    assert_eq!(of_fs_type(0xfc00, 4), Licence::Free);
    assert_eq!(of_fs_type(0x0003, 4), Licence::Refused(Refusal::Restricted), "the reserved bit 0 does not make it more permissive");
    // a table of version 0 or 1 has no bits 8 and 9
    for version in [0, 1] {
        assert_eq!(of_fs_type(0x0200, version), Licence::Free, "version {version}");
        assert_eq!(of_fs_type(0x0100, version), Licence::Free, "version {version}");
    }
    assert_eq!(of_fs_type(0x0200, 2), Licence::Refused(Refusal::BitmapOnly));
    assert_eq!(of_fs_type(0x0100, 3), Licence::Whole);
}

// spec: PDF-038
#[test]
fn pdf_038_the_licence_is_read_from_the_os2_table_of_a_font_and_a_font_without_one_is_installable() {
    assert_eq!(licence_of(Spec::new("Tiny", 400, false)), Licence::Free);
    assert_eq!(licence_of(Spec::new("Tiny", 400, false).licence(0x0002, 3)), Licence::Refused(Refusal::Restricted));
    assert_eq!(licence_of(Spec::new("Tiny", 400, false).licence(0x0200, 4)), Licence::Refused(Refusal::BitmapOnly));
    assert_eq!(licence_of(Spec::new("Tiny", 400, false).licence(0x0100, 4)), Licence::Whole);
    assert_eq!(licence_of(Spec::new("Tiny", 400, false).licence(0x0004, 1)), Licence::Free);
    assert_eq!(licence_of(Spec::new("Tiny", 400, false).licence(0x0002, 0)), Licence::Refused(Refusal::Restricted), "a version 0 table");
    assert_eq!(licence_of(Spec::new("Tiny", 400, false).licence(0x0002, 4).without_os2()), Licence::Free, "no OS/2 table");
    // a bundled font has a licence file beside it and is installable
    let bytes = include_bytes!("../../assets/fonts/LiberationSerif-Regular.ttf");
    assert_eq!(of_font(&FontRef::new(bytes.as_slice()).unwrap()), Licence::Free);
}

// spec: PDF-038
#[test]
fn pdf_038_a_refusal_says_why() {
    assert_eq!(Refusal::Restricted.to_string(), "its licence does not allow embedding it");
    assert_eq!(Refusal::BitmapOnly.to_string(), "its licence allows only bitmaps to be embedded");
}
