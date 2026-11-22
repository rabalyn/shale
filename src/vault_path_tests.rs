//! Property tests of the typed vault path (VAULT-018, DEV-042): what the checking constructors
//! accept never leaves the vault, whatever is typed. The example tests are in `vault_path.rs`.

use std::path::{Component, Path};

use proptest::prelude::*;

use crate::property_test_support::{config, config_with, path_text};
use crate::vault::Vault;
use crate::vault_path::VaultPath;

/// Every way a path can leave the vault, spelled out so that it holds on Linux too, where
/// `Path` knows no drive or UNC prefix: only plain, non-empty names, none with a separator or a
/// drive colon, and the whole is not absolute.
fn stays_inside(p: &Path) -> Result<(), String> {
    if p.is_absolute() || p.has_root() {
        return Err(format!("absolute: {}", p.display()));
    }
    for c in p.components() {
        let Component::Normal(n) = c else { return Err(format!("not a plain name ({c:?}) in {}", p.display())) };
        let n = n.to_string_lossy();
        if n.is_empty() || n == ".." || n == "." || n.contains(['/', '\\', ':']) || n.trim() != n {
            return Err(format!("odd part {n:?} in {}", p.display()));
        }
    }
    Ok(())
}

/// The typed form of a checked path: its names with `/`.
fn typed(p: &VaultPath) -> String {
    p.as_path().components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/")
}

/// Whether nothing is left of `text` once the empty, blank and `.` parts are dropped (the root).
fn says_root(text: &str) -> bool {
    text.trim().split(['/', '\\']).all(|p| matches!(p.trim(), "" | "."))
}

proptest! {
    #![proptest_config(config())]

    // spec: VAULT-018
    #[test]
    fn vault_018_a_parsed_path_never_leaves_the_vault(text in path_text()) {
        if let Ok(p) = VaultPath::parse(&text) {
            prop_assert_eq!(stays_inside(p.as_path()), Ok(()));
            // the root only when nothing was typed
            prop_assert_eq!(p.as_path().as_os_str().is_empty(), says_root(&text), "{:?}", p);
        }
    }

    // spec: VAULT-018
    #[test]
    fn vault_018_a_checked_name_is_one_plain_name(text in path_text()) {
        if let Ok(n) = VaultPath::name(&text) {
            prop_assert!(!n.is_empty());
            prop_assert_eq!(stays_inside(Path::new(&n)), Ok(()));
            prop_assert_eq!(Path::new(&n).components().count(), 1, "{:?}", n);
        }
    }

    // spec: VAULT-018
    #[test]
    fn vault_018_a_joined_path_never_leaves_the_vault(base in path_text(), text in path_text()) {
        let Ok(base) = VaultPath::parse(&base) else { return Ok(()) };
        if let Ok(p) = base.join_name(&text) {
            prop_assert_eq!(stays_inside(p.as_path()), Ok(()));
            prop_assert!(!p.as_path().as_os_str().is_empty(), "a name is never the root");
            prop_assert!(p.as_path().starts_with(base.as_path()));
        }
        if let Ok(p) = base.join(&text) {
            prop_assert_eq!(stays_inside(p.as_path()), Ok(()));
            prop_assert!(p.as_path().starts_with(base.as_path()));
        }
    }

    // spec: VAULT-018
    #[test]
    fn vault_018_parsing_a_result_again_gives_the_same_path(text in path_text()) {
        if let Ok(p) = VaultPath::parse(&text) {
            prop_assert_eq!(VaultPath::parse(&typed(&p)), Ok(p));
        }
        if let Ok(n) = VaultPath::name(&text) {
            prop_assert_eq!(VaultPath::name(&n), Ok(n));
        }
    }
}

proptest! {
    // each case checks a path against a real folder, so fewer of them
    #![proptest_config(config_with(256))]

    // spec: VAULT-018
    #[test]
    fn vault_018_the_last_guard_agrees_with_the_path_type(text in path_text()) {
        let d = std::env::temp_dir().join(format!("shale-test-vault-path-prop-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let vault = Vault::open(&d).unwrap();
        // a raw path as code hands it over: accepted only without `..` and without a root
        let raw = Path::new(&text);
        if let Ok(abs) = vault.abs_checked(raw) {
            prop_assert_eq!(no_way_out(raw), Ok(()));
            prop_assert_eq!(abs, vault.abs(raw));
        }
        // a checked path: accepted, except the root
        let typed_path = VaultPath::parse(&text);
        if let Ok(p) = typed_path {
            if p.as_path().as_os_str().is_empty() {
                prop_assert!(vault.abs_checked(p.as_path()).is_err(), "the root is refused");
            } else {
                prop_assert!(vault.abs_checked(p.as_path()).is_ok(), "{:?} is accepted", p);
                prop_assert!(vault.abs_checked(p.as_path()).unwrap().starts_with(&d));
            }
        }
    }
}

/// Whether a raw path has no `..`, no root and no drive.
fn no_way_out(p: &Path) -> Result<(), String> {
    for c in p.components() {
        if matches!(c, Component::ParentDir | Component::RootDir | Component::Prefix(_)) {
            return Err(format!("{c:?} in {}", p.display()));
        }
    }
    Ok(())
}
