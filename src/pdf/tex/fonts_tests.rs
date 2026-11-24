//! The fonts of the LaTeX export written from a family that was found (TEX-054, TEX-055): the
//! cases that a document made through the writer (`writer_tests.rs`) does not reach, a face of a
//! font collection, a file that cannot be read, and the names of the copies.

use super::fonts::*;
use crate::fonts::resolve::{Family, FontFile, Role};
use crate::fonts::scan::Place;
use std::path::{Path, PathBuf};

fn family(files: [Option<(&Path, u32)>; 4]) -> Family {
    Family { name: "Garamond".into(), place: Place::Vault, faces: files.map(|f| f.map(|(path, index)| FontFile { path: path.to_path_buf(), index })), whole: false }
}

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("shale-texfonts-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

// spec: TEX-055
#[test]
fn tex_055_a_face_of_a_font_collection_beyond_the_first_cannot_be_selected_and_says_so() {
    let dir = scratch("ttc");
    let file = dir.join("Garamond.ttc");
    std::fs::write(&file, b"x").unwrap();
    let mut copies = Copies::default();
    let first = from_files(Role::Text, &family([Some((&file, 0)), None, None, None]), &mut copies).unwrap();
    assert_eq!(first.line, "\\setmainfont{Garamond.ttc}[Path=./]", "the first face of a collection is what fontspec loads");
    let second = from_files(Role::Text, &family([Some((&file, 0)), Some((&file, 1)), None, None]), &mut copies).unwrap_err();
    assert_eq!(second, "the font “Garamond” is the face 1 of a font collection, which TeX cannot select");
    let _ = std::fs::remove_dir_all(dir);
}

// spec: TEX-055
#[test]
fn tex_055_a_font_file_that_cannot_be_read_is_an_error_for_the_person_not_a_missing_file_in_tex() {
    let dir = scratch("gone");
    let gone = dir.join("Gone Font.otf");
    let e = from_files(Role::Sans, &family([Some((&gone, 0)), None, None, None]), &mut Copies::default()).unwrap_err();
    assert!(e.starts_with("the font file “Gone Font.otf” could not be read ("), "{e}");
    let _ = std::fs::remove_dir_all(dir);
}

// spec: TEX-055
#[test]
fn tex_055_a_family_without_a_regular_face_is_refused() {
    let dir = scratch("noregular");
    let bold = dir.join("B.ttf");
    std::fs::write(&bold, b"x").unwrap();
    let e = from_files(Role::Mono, &family([None, Some((&bold, 0)), None, None]), &mut Copies::default()).unwrap_err();
    assert_eq!(e, "the font “Garamond” has no regular face");
    let _ = std::fs::remove_dir_all(dir);
}

// spec: TEX-055
#[test]
fn tex_055_the_commands_are_the_roles_and_the_options_are_the_faces() {
    let dir = scratch("roles");
    let paths: Vec<PathBuf> = ["R.otf", "B.otf", "I.otf", "Z.otf"].iter().map(|n| dir.join(n)).collect();
    for p in &paths {
        std::fs::write(p, p.file_name().unwrap().as_encoded_bytes()).unwrap();
    }
    let all = family([Some((&paths[0], 0)), Some((&paths[1], 0)), Some((&paths[2], 0)), Some((&paths[3], 0))]);
    let mut copies = Copies::default();
    let lines: Vec<String> = Role::ALL.iter().map(|r| from_files(*r, &all, &mut copies).unwrap().line).collect();
    let options = "[Path=./, BoldFont=B.otf, ItalicFont=I.otf, BoldItalicFont=Z.otf]";
    assert_eq!(lines, [format!("\\setmainfont{{R.otf}}{options}"), format!("\\setsansfont{{R.otf}}{options}"), format!("\\setmonofont{{R.otf}}{options}")]);
    let _ = std::fs::remove_dir_all(dir);
}
