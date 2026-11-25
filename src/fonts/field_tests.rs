//! The font fields of the exports (PDF-039, CFG-080): the list of families and the line that says
//! what a value comes to. The fields themselves are drawn in the export dialog and in Settings,
//! whose tests click them.

use super::field::{check, family_names};
use super::resolve::Role;
use super::scan::{scan_system, scan_vault, Catalogue};
use super::test_fonts::{folder, Spec};
use std::sync::Arc;

fn catalogue(root: &std::path::Path, sys: &std::path::Path, ready: bool) -> Catalogue {
    Catalogue { vault: Arc::new(scan_vault(root)), system: Arc::new(scan_system(Some(std::slice::from_ref(&sys.to_path_buf())))), system_ready: ready, generation: 1 }
}

// spec: PDF-039
#[test]
fn pdf_039_the_list_has_each_family_once_in_alphabetical_order_from_the_vault_and_the_system() {
    let (root, sys) = (folder("field-list"), folder("field-list-sys"));
    Spec::new("Zeta Serif", 400, false).write(&root, "fonts/Zeta.ttf");
    Spec::new("Alpha Sans", 400, false).write(&sys, "Alpha.ttf");
    Spec::new("Alpha Sans", 700, false).write(&sys, "AlphaBold.ttf");
    Spec::new("alpha-sans", 400, true).write(&sys, "AlphaIt.ttf");
    Spec::new("Mid Mono", 400, false).write(&sys, "Mid.ttf");
    assert_eq!(family_names(&catalogue(&root, &sys, true)), ["Alpha Sans", "Mid Mono", "Zeta Serif"]);
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&sys);
}

// spec: CFG-080
#[test]
fn cfg_080_a_value_says_where_it_is_found_or_why_it_cannot_be_used() {
    let (root, sys) = (folder("field-check"), folder("field-check-sys"));
    Spec::new("Vaulty", 400, false).write(&root, "fonts/Vaulty.ttf");
    Spec::new("Systemy", 400, false).write(&sys, "Systemy.ttf");
    Spec::new("Locked", 400, false).licence(0x0002, 4).write(&sys, "Locked.ttf");
    let cat = catalogue(&root, &sys, true);
    let say = |role, v: &str| check(role, v, &root, &cat);
    assert_eq!(say(Role::Text, "  "), None, "an empty field says nothing");
    let found = say(Role::Text, "vaulty").unwrap();
    assert!(!found.problem && found.text.starts_with("Found: Vaulty (vault"), "{found:?}");
    let found = say(Role::Mono, "Systemy").unwrap();
    assert!(!found.problem && found.text.contains("Systemy (system"), "{found:?}");
    let word = say(Role::Text, "serif").unwrap();
    assert!(!word.problem && word.text.contains("Liberation Serif") && word.text.contains("built into the PDF"), "{word:?}");
    let tex = say(Role::Sans, "XCharter").unwrap();
    assert!(!tex.problem && tex.text.contains("TeX Live"), "{tex:?}");
    let refused = say(Role::Text, "Locked").unwrap();
    assert!(refused.problem && refused.text.contains("Locked") && refused.text.contains("print_font"), "{refused:?}");
    let missing = say(Role::Text, "fonts/Nothing.ttf").unwrap();
    assert!(missing.problem && missing.text.contains("was not found"), "{missing:?}");
    // while the installed fonts are being looked up, a name that the vault lacks is not known to be missing
    let early = Catalogue { system: Arc::default(), ..catalogue(&root, &sys, false) };
    assert_eq!(check(Role::Text, "Systemy", &root, &early).map(|c| (c.problem, c.text)), Some((false, "Looking up fonts...".into())));
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&sys);
}
