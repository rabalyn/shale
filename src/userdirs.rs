//! The user's own folders (settings, data, cache, home, documents): the only place in the app
//! that asks the `dirs` crate for them (DEV-047). In a test build none of them is the person's
//! real folder: they all lie in one folder of this test process below the temp dir, so a test
//! that saves the settings, the mail store, a cache or anything else on any thread can never
//! overwrite the owner's real files (raba/shale#685). A test that wants its own place sets one
//! (`config::testing::TestConfig` for the settings file) and gets it on its thread; every other
//! thread of the process shares this folder, which is the safe default.

use std::path::PathBuf;

/// The folder of this test process: nothing in it is ever the person's.
#[cfg(test)]
fn sandbox() -> PathBuf {
    static ROOT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    ROOT.get_or_init(|| std::env::temp_dir().join(format!("shale-test-home-{}", std::process::id()))).clone()
}

macro_rules! user_dir {
    ($(#[$doc:meta])* $name:ident, $dirs:path, $sub:literal) => {
        $(#[$doc])*
        pub fn $name() -> Option<PathBuf> {
            #[cfg(test)]
            {
                Some(sandbox().join($sub))
            }
            #[cfg(not(test))]
            {
                $dirs()
            }
        }
    };
}

user_dir! {
    /// Where the settings live (`~/.config` on Linux).
    config_dir, dirs::config_dir, "config"
}
user_dir! {
    /// Where the app's data lives (`~/.local/share` on Linux).
    data_dir, dirs::data_dir, "data"
}
user_dir! {
    /// Where caches live (`~/.cache` on Linux).
    cache_dir, dirs::cache_dir, "cache"
}
user_dir! {
    /// The person's home folder.
    home_dir, dirs::home_dir, "home"
}
user_dir! {
    /// The person's documents folder.
    document_dir, dirs::document_dir, "documents"
}

/// The folder for the person's runtime files (`$XDG_RUNTIME_DIR`). A test has none: callers then
/// use the temp folder.
pub fn runtime_dir() -> Option<PathBuf> {
    #[cfg(test)]
    {
        None
    }
    #[cfg(not(test))]
    {
        dirs::runtime_dir()
    }
}

/// Windows tests only: the real profile folders, to check where Windows puts them (DATA-014).
/// Reading their names touches nothing.
#[cfg(all(test, windows))]
pub fn real_folders() -> (PathBuf, PathBuf, PathBuf) {
    (dirs::home_dir().unwrap(), dirs::config_dir().unwrap(), dirs::data_dir().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec: DEV-047
    #[test]
    fn dev_047_in_a_test_no_user_folder_is_the_real_one() {
        let real = [dirs::config_dir(), dirs::data_dir(), dirs::cache_dir(), dirs::home_dir(), dirs::document_dir()];
        let fake = [config_dir(), data_dir(), cache_dir(), home_dir(), document_dir()];
        for (real, fake) in real.iter().zip(&fake) {
            let (Some(real), Some(fake)) = (real, fake) else { continue };
            assert!(!fake.starts_with(real) && !real.starts_with(fake), "{} is the real {}", fake.display(), real.display());
            assert!(fake.starts_with(std::env::temp_dir()), "{} is not in the temp dir", fake.display());
        }
        // the settings file of a thread that set no path of its own
        let cfg = crate::config::config_path().unwrap();
        assert!(cfg.starts_with(sandbox()), "the settings file of a test is not in the test folder: {}", cfg.display());
    }

    // spec: DEV-047
    #[test]
    fn dev_047_only_this_file_asks_the_dirs_crate_for_a_folder() {
        fn walk(dir: &std::path::Path, found: &mut Vec<String>) {
            for e in std::fs::read_dir(dir).unwrap() {
                let p = e.unwrap().path();
                if p.is_dir() {
                    walk(&p, found);
                } else if p.extension().is_some_and(|x| x == "rs") && !p.ends_with("userdirs.rs") {
                    for (n, line) in std::fs::read_to_string(&p).unwrap().lines().enumerate() {
                        let code = line.split("//").next().unwrap_or("");
                        let call = code.match_indices("dirs::").any(|(i, _)| !code[..i].ends_with(|c: char| c.is_alphanumeric() || c == '_'));
                        if call {
                            found.push(format!("{}:{}", p.display(), n + 1));
                        }
                    }
                }
            }
        }
        let mut found = Vec::new();
        walk(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut found);
        assert!(found.is_empty(), "use crate::userdirs instead of the dirs crate (DEV-047): {found:?}");
    }
}
