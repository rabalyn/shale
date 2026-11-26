//! The scan of the fonts of the vault and the system (JOB-040, ADR-0040 §8): the folders it
//! reads, the faces it records, and the background job with its cache. The system is a test
//! folder, so the tests do not depend on the machine.

use super::licence::{Licence, Refusal};
use super::scan::{faces_of_file, name_key, scan_system, scan_vault, signature, Catalogue, FontScan, Place, FONT_SCAN};
use super::test_fonts::{folder, Spec};
use crate::busy::Busy;
use crate::vault::Vault;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn families(faces: &[super::scan::Face]) -> Vec<String> {
    let mut v: Vec<String> = faces.iter().map(|f| f.families[0].clone()).collect();
    v.sort();
    v
}

// spec: PDF-035
#[test]
fn pdf_035_family_names_are_compared_without_case_spaces_hyphens_and_underscores() {
    assert_eq!(name_key("Libertinus Serif"), name_key("libertinus-serif"));
    assert_eq!(name_key("Libertinus Serif"), name_key("LIBERTINUS_SERIF"));
    assert_eq!(name_key("Libertinus  Serif"), name_key("LibertinusSerif"));
    assert_ne!(name_key("Libertinus Serif"), name_key("Libertinus"));
}

// spec: PDF-035
#[test]
fn pdf_035_the_vault_scan_reads_fonts_at_any_depth_not_dot_folders_and_the_sets_own_fonts() {
    let root = folder("scan-vault");
    Spec::new("Top", 400, false).write(&root, "fonts/Top.ttf");
    Spec::new("Deep", 400, false).write(&root, "fonts/a/b/Deep.otf");
    Spec::new("Hidden", 400, false).write(&root, "fonts/.cache/Hidden.ttf");
    Spec::new("Outside", 400, false).write(&root, "elsewhere/Outside.ttf");
    Spec::new("Setfont", 400, false).write(&root, "templates/tex/uni/fonts/Setfont.ttf");
    Spec::new("Refused", 400, false).licence(0x0002, 4).write(&root, "fonts/Refused.ttf");
    std::fs::write(root.join("fonts/notes.txt"), "not a font").unwrap();
    std::fs::write(root.join("fonts/Broken.ttf"), "not a font either").unwrap();
    let faces = scan_vault(&root);
    assert_eq!(families(&faces), ["Deep", "Refused", "Setfont", "Top"]);
    let place = |name: &str| faces.iter().find(|f| f.families[0] == name).unwrap().place.clone();
    assert_eq!((place("Top"), place("Deep")), (Place::Vault, Place::Vault));
    assert_eq!(place("Setfont"), Place::Set("uni".into()));
    let refused = faces.iter().find(|f| f.families[0] == "Refused").unwrap();
    assert_eq!(refused.licence, Licence::Refused(Refusal::Restricted), "the scan records the licence");
    let _ = std::fs::remove_dir_all(&root);
}

// spec: PDF-035
#[test]
fn pdf_035_a_face_has_its_names_weight_slant_and_file() {
    let d = folder("scan-face");
    let path = Spec::new("Slanted Face", 700, true).write(&d, "S.ttf");
    let faces = faces_of_file(&path, &Place::Vault);
    assert_eq!(faces.len(), 1);
    let f = &faces[0];
    assert_eq!((f.families.first().map(String::as_str), f.weight, f.italic, f.index, &f.path), (Some("Slanted Face"), 700, true, 0, &path));
    assert!(faces_of_file(&d.join("missing.ttf"), &Place::Vault).is_empty());
    std::fs::write(d.join("x.ttf"), "nope").unwrap();
    assert!(faces_of_file(&d.join("x.ttf"), &Place::Vault).is_empty(), "a file that is no font has no faces");
    let _ = std::fs::remove_dir_all(&d);
}

// spec: JOB-040
#[test]
fn job_040_the_system_scan_reads_the_folders_it_is_given() {
    let sys = folder("scan-system");
    Spec::new("Sysfont", 400, false).write(&sys, "a/Sysfont.ttf");
    let faces = scan_system(Some(std::slice::from_ref(&sys)));
    assert_eq!((families(&faces), faces[0].place.clone()), (vec!["Sysfont".to_string()], Place::System));
    let _ = std::fs::remove_dir_all(&sys);
}

fn vault_in(root: &std::path::Path) -> Vault {
    std::fs::create_dir_all(root).unwrap();
    Vault::open(root).unwrap()
}

/// Tick until the scan has ended (its job is on another thread).
fn finish(scan: &mut FontScan, vault: Option<&Vault>, busy: &Busy) {
    let start = Instant::now();
    while scan.running() {
        assert!(start.elapsed() < Duration::from_secs(20), "the scan did not end");
        std::thread::sleep(Duration::from_millis(5));
        scan.tick(vault, busy, || {});
    }
}

// spec: JOB-040
#[test]
fn job_040_a_scan_is_a_task_that_the_registry_refuses_twice_and_the_result_is_cached() {
    let root = folder("scan-job");
    let sys = folder("scan-job-sys");
    Spec::new("Sysfont", 400, false).write(&sys, "Sysfont.ttf");
    Spec::new("Vaultfont", 400, false).write(&root, "fonts/Vaultfont.ttf");
    let vault = vault_in(&root);
    let busy = Busy::default();
    let mut scan = FontScan::default().with_system_dirs(vec![sys.clone()]);
    // nothing asks for the fonts: nothing is looked up
    scan.tick(Some(&vault), &busy, || {});
    assert!(!scan.running() && !scan.catalogue().system_ready);
    scan.request();
    // while a task of the key runs, none starts (JOB-005)
    let held = busy.begin(FONT_SCAN, "held").unwrap();
    scan.tick(Some(&vault), &busy, || {});
    assert!(!scan.running(), "the registry refused a second scan");
    drop(held);
    // the scan is a quiet task: running at once, listed in the indicator only after two seconds
    let gate = scan.hold();
    scan.tick(Some(&vault), &busy, || {});
    assert!(scan.running());
    let info = busy.info(&FONT_SCAN).expect("the task is registered");
    assert_eq!(info.label, "Looking up fonts...");
    assert!(busy.list_at(Instant::now()).is_empty(), "not listed at first");
    assert!(busy.list_at(Instant::now() + Duration::from_secs(3)).iter().any(|t| t.label == "Looking up fonts..."), "listed after 2 s");
    gate.store(true, std::sync::atomic::Ordering::SeqCst);
    finish(&mut scan, Some(&vault), &busy);
    assert!(!busy.is_running(&FONT_SCAN), "the task ended with the job");
    let cat: Catalogue = scan.catalogue();
    assert!(cat.system_ready);
    assert_eq!((families(&cat.system), families(&cat.vault)), (vec!["Sysfont".to_string()], vec!["Vaultfont".to_string()]));
    // cached: another tick does not look again
    let generation = cat.generation;
    scan.tick(Some(&vault), &busy, || {});
    assert!(!scan.running(), "nothing changed, nothing is scanned");
    assert_eq!(scan.catalogue().generation, generation);
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&sys);
}

// spec: JOB-040
#[test]
fn job_040_the_vaults_fonts_are_read_again_when_its_font_files_change_and_the_system_is_not() {
    let root = folder("scan-change");
    let sys = folder("scan-change-sys");
    Spec::new("Sysfont", 400, false).write(&sys, "Sysfont.ttf");
    let mut vault = vault_in(&root);
    let busy = Busy::default();
    let mut scan = FontScan::default().with_system_dirs(vec![sys.clone()]);
    scan.request();
    scan.tick(Some(&vault), &busy, || {});
    finish(&mut scan, Some(&vault), &busy);
    assert!(scan.catalogue().vault.is_empty());
    let system: Arc<Vec<_>> = scan.catalogue().system;
    let before = signature(&vault);
    // a note is no font: the signature stays and nothing is scanned
    std::fs::write(root.join("Note.md"), "# Note\n").unwrap();
    vault.rescan();
    assert_eq!(signature(&vault), before);
    scan.tick(Some(&vault), &busy, || {});
    assert!(!scan.running());
    // a font file in fonts/ is: the vault's part is read again, the system's is not
    Spec::new("Vaultfont", 400, false).write(&root, "fonts/Vaultfont.ttf");
    vault.rescan();
    assert_ne!(signature(&vault), before);
    scan.tick(Some(&vault), &busy, || {});
    assert!(scan.running());
    finish(&mut scan, Some(&vault), &busy);
    assert_eq!(families(&scan.catalogue().vault), ["Vaultfont"]);
    assert!(Arc::ptr_eq(&system, &scan.catalogue().system), "the system fonts were not read again");
    // a font outside fonts/ is no font of the exports
    Spec::new("Elsewhere", 400, false).write(&root, "pictures/Elsewhere.ttf");
    vault.rescan();
    scan.tick(Some(&vault), &busy, || {});
    assert!(!scan.running());
    // the vault closes: its fonts go
    scan.tick(None, &busy, || {});
    assert!(scan.catalogue().vault.is_empty());
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&sys);
}

// spec: CFG-081, JOB-040
#[test]
fn cfg_081_a_scan_on_request_reads_the_system_and_the_vault_anew_and_keeps_the_old_result_until_it_is_in() {
    let root = folder("scan-again");
    let sys = folder("scan-again-sys");
    Spec::new("Sysfont", 400, false).write(&sys, "Sysfont.ttf");
    let vault = vault_in(&root);
    let busy = Busy::default();
    let mut scan = FontScan::default().with_system_dirs(vec![sys.clone()]);
    scan.request();
    scan.tick(Some(&vault), &busy, || {});
    finish(&mut scan, Some(&vault), &busy);
    assert_eq!(families(&scan.catalogue().system), ["Sysfont"]);
    // a font installed while Shale runs is not seen by itself
    Spec::new("Newfont", 400, false).write(&sys, "Newfont.ttf");
    scan.tick(Some(&vault), &busy, || {});
    assert!(!scan.running());
    assert_eq!(families(&scan.catalogue().system), ["Sysfont"]);
    // asked for again: a scan runs, the old result stays in effect until the new one is in
    scan.rescan();
    scan.tick(Some(&vault), &busy, || {});
    assert!(scan.running() && scan.catalogue().system_ready);
    assert_eq!(families(&scan.catalogue().system), ["Sysfont"]);
    // a second request while it runs starts no second scan, and none follows it
    scan.rescan();
    finish(&mut scan, Some(&vault), &busy);
    assert_eq!(families(&scan.catalogue().system), ["Newfont", "Sysfont"]);
    scan.tick(Some(&vault), &busy, || {});
    assert!(!scan.running(), "one request, one scan");
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&sys);
}
