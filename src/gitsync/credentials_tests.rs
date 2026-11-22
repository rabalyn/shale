//! Tests of the askpass mechanism's parts (SYNC-053, SYNC-054); the executable itself runs in
//! `tests/askpass.rs`.

use super::credentials::*;
use super::Git;
use crate::secret::Secret;
use std::ffi::OsStr;
use std::path::Path;

// spec: SYNC-054
#[test]
fn sync_054_servers_and_users_are_read_from_addresses_and_prompts() {
    assert_eq!(server_key("https://Git.Example.org/o/r.git").as_deref(), Some("https://git.example.org"));
    assert_eq!(server_key("https://ada@git.example.org:443/o/r.git").as_deref(), Some("https://git.example.org"));
    assert_eq!(server_key("http://127.0.0.1:3000/o/r").as_deref(), Some("http://127.0.0.1:3000"));
    assert_eq!(server_key("http://localhost:80/").as_deref(), Some("http://localhost"));
    for bad in ["git@host:o/r.git", "ssh://host/r", "/srv/repo.git", "https://", ""] {
        assert_eq!(server_key(bad), None, "{bad}");
    }
    assert_eq!(user_of("https://ada@git.test/o/r.git").as_deref(), Some("ada"));
    assert_eq!(user_of("https://git.test/o/r.git"), None);
    assert_eq!(user_of("https://@git.test/"), None);

    let p = parse_prompt("Password for 'https://ada@git.test:8443': ").unwrap();
    assert_eq!(p, Prompt { password: true, server: "https://git.test:8443".into(), user: Some("ada".into()) });
    let p = parse_prompt("Username for 'https://git.test': ").unwrap();
    assert_eq!(p, Prompt { password: false, server: "https://git.test".into(), user: None });
    for bad in ["Enter passphrase for key '/home/u/.ssh/id_ed25519': ", "Password: ", "Password for 'ssh://h': ", ""] {
        assert_eq!(parse_prompt(bad), None, "{bad}");
    }
}

// spec: SYNC-054
#[test]
fn sync_054_only_a_password_prompt_with_a_user_and_a_stored_token_is_answered() {
    let lookup = |server: &str, user: &str| (server == "https://git.test" && user == "ada").then(|| Secret::new("tok"));
    assert_eq!(answer("Password for 'https://ada@git.test': ", lookup).map(|s| s.expose().to_string()), Some("tok".into()));
    assert_eq!(answer("Password for 'https://ada@GIT.TEST:443': ", lookup).map(|s| s.expose().to_string()), Some("tok".into()), "host case and the default port do not matter");
    assert!(answer("Password for 'https://bob@git.test': ", lookup).is_none());
    assert!(answer("Password for 'https://ada@other.test': ", lookup).is_none());
    assert!(answer("Password for 'https://git.test': ", lookup).is_none(), "no user, no token");
    assert!(answer("Username for 'https://git.test': ", lookup).is_none());
    assert!(answer("something else", lookup).is_none());
}

// spec: SYNC-054
#[test]
fn sync_054_shale_is_the_helper_only_with_the_marker_and_one_prompt_argument() {
    let prompt = vec!["Password for 'https://ada@git.test': ".to_string()];
    assert!(askpass_requested(Some(OsStr::new("1")), &prompt));
    assert!(!askpass_requested(None, &prompt));
    assert!(!askpass_requested(Some(OsStr::new("0")), &prompt));
    assert!(!askpass_requested(Some(OsStr::new("1")), &[]));
    assert!(!askpass_requested(Some(OsStr::new("1")), &["/home/u/Notes".to_string()]), "a vault path is never a prompt");
    assert!(!askpass_requested(Some(OsStr::new("1")), &[prompt[0].clone(), "x".to_string()]));
}

// spec: SYNC-054
#[test]
fn sync_054_attach_sets_the_helper_and_keeps_the_rest_of_the_environment() {
    let mut git = Git { program: "git".into(), env: vec![("A".into(), "1".into()), ("GIT_ASKPASS".into(), "old".into())] };
    attach(&mut git, Path::new("/opt/shale"));
    let get = |k: &str| git.env.iter().filter(|(key, _)| key == k).map(|(_, v)| v.to_string_lossy().into_owned()).collect::<Vec<_>>();
    assert_eq!(get("A"), ["1"]);
    assert_eq!(get("GIT_ASKPASS"), ["/opt/shale"]);
    assert_eq!(get(ASKPASS_ENV), ["1"]);
    assert_eq!(get("GCM_INTERACTIVE"), ["never"]);
    // twice is the same
    attach(&mut git, Path::new("/opt/shale"));
    assert_eq!(git.env.len(), 4);
}
