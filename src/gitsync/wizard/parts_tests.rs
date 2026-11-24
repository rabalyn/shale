//! Tests of the wizard's parts: providers, the API calls (a pretend server), SSH keys (the real
//! `ssh-keygen`), what the remote holds and the words of the results (SYNC-051…SYNC-058).

use super::api::{self, ApiError};
use super::provider::{self, Kind, Server};
use super::ssh;
use super::test_sync::{self, Problem, Remote};
use crate::calendar::dav::tests::{closed_port, mock_http};
use crate::gitsync::tests::{write, Env};
use crate::secret::Secret;
use std::path::Path;

fn server(kind: Kind, host: &str, scheme: &str) -> Server {
    Server { kind, scheme: scheme.into(), host: host.into() }
}

// spec: SYNC-051
#[test]
fn sync_051_hosts_and_addresses_are_read_into_server_kind_and_repository() {
    let p = provider::parse("github.com", Kind::Other).unwrap();
    assert_eq!(p.server, server(Kind::GitHub, "github.com", "https"));
    assert!(p.repo.is_none() && !p.ssh);
    let p = provider::parse("https://github.com/ada/notes.git", Kind::Other).unwrap();
    assert_eq!((p.server.kind, p.repo), (Kind::GitHub, Some(("ada".into(), "notes".into()))));
    let p = provider::parse("git@github.com:ada/notes.git", Kind::Other).unwrap();
    assert_eq!((p.server.kind, p.repo, p.ssh), (Kind::GitHub, Some(("ada".into(), "notes".into())), true));
    let p = provider::parse("ssh://git@git.example.org/ada/notes", Kind::Forgejo).unwrap();
    assert_eq!((p.server.host.as_str(), p.server.kind, p.repo, p.ssh), ("git.example.org", Kind::Forgejo, Some(("ada".into(), "notes".into())), true));
    // a host that is not known takes the kind the person picked; a user name in the address is dropped
    let p = provider::parse("https://ada@Git.Example.org:8443/o/r", Kind::Forgejo).unwrap();
    assert_eq!((p.server, p.repo), (server(Kind::Forgejo, "git.example.org:8443", "https"), Some(("o".into(), "r".into()))));
    assert_eq!(provider::parse("gitlab.com/group/sub/proj", Kind::Other).unwrap().repo, Some(("group/sub".into(), "proj".into())));
    assert_eq!(provider::parse("codeberg.org", Kind::GitHub).unwrap().server.kind, Kind::Forgejo, "a known host wins over the pick");
    // a server on this computer may be plain http
    assert_eq!(provider::parse("http://127.0.0.1:3000", Kind::Forgejo).unwrap().server, server(Kind::Forgejo, "127.0.0.1:3000", "http"));
    for bad in ["", "   ", "http://example.org", "https://ada:secret@example.org/o/r", "ssh://", "git@:x", "ftp://example.org", "has space.org"] {
        assert!(provider::parse(bad, Kind::Other).is_err(), "{bad:?}");
    }
}

// spec: SYNC-051, SYNC-052, SYNC-055, SYNC-056
#[test]
fn sync_052_each_provider_has_its_token_key_and_repository_pages_and_remote_addresses() {
    let gh = server(Kind::GitHub, "github.com", "https");
    assert_eq!(gh.api().as_deref(), Some("https://api.github.com"));
    assert!(gh.token_page().unwrap().starts_with("https://github.com/settings/tokens/new?scopes=repo"));
    assert_eq!(gh.ssh_keys_page().as_deref(), Some("https://github.com/settings/ssh/new"));
    assert_eq!(gh.new_repo_page().as_deref(), Some("https://github.com/new"));
    assert_eq!(server(Kind::GitHub, "ghe.corp.org", "https").api().as_deref(), Some("https://ghe.corp.org/api/v3"));
    let cb = server(Kind::Forgejo, "codeberg.org", "https");
    assert_eq!(cb.api().as_deref(), Some("https://codeberg.org/api/v1"));
    assert_eq!(cb.token_page().as_deref(), Some("https://codeberg.org/user/settings/applications"));
    assert_eq!(cb.ssh_keys_page().as_deref(), Some("https://codeberg.org/user/settings/keys"));
    let gl = server(Kind::GitLab, "gitlab.com", "https");
    assert!(gl.api().is_none() && !gl.has_api());
    assert!(gl.token_page().unwrap().contains("/-/user_settings/personal_access_tokens"));
    assert_eq!(gl.new_repo_page().as_deref(), Some("https://gitlab.com/projects/new"));
    let other = server(Kind::Other, "git.example.org", "https");
    assert!(other.token_page().is_none() && other.ssh_keys_page().is_none() && other.new_repo_page().is_none() && other.api().is_none());
    // the token is never in an address: the user name is
    assert_eq!(cb.https_remote("ada", "ada", "notes"), "https://ada@codeberg.org/ada/notes.git");
    assert_eq!(server(Kind::Forgejo, "git.x.org:8443", "https").ssh_remote("ada", "notes"), "git@git.x.org:ada/notes.git");
    assert!(crate::gitsync::check_url(&cb.https_remote("ada", "ada", "notes")).is_ok());
}

fn forgejo(base: &str) -> Server {
    server(Kind::Forgejo, base.trim_start_matches("http://"), "http")
}

const REPOS: &str = r#"[
 {"name":"notes","full_name":"ada/notes","private":true,"empty":true,"owner":{"login":"ada"}},
 {"name":"work","full_name":"ada/work","private":false,"empty":false,"owner":{"login":"ada"}}]"#;

// spec: SYNC-055
#[test]
fn sync_055_the_api_names_the_user_lists_and_creates_repositories_with_the_token_in_a_header() {
    let token = Secret::new("tok-123");
    let (base, seen) = mock_http(3, |method, path, headers, body| {
        assert!(headers.to_ascii_lowercase().contains("authorization: token tok-123"), "{headers}");
        match (method, path) {
            ("GET", "/api/v1/user") => (200, r#"{"login":"ada","id":1}"#.into()),
            ("GET", "/api/v1/user/repos?limit=50") => (200, REPOS.into()),
            ("POST", "/api/v1/user/repos") => {
                let v: serde_json::Value = serde_json::from_str(body).unwrap();
                assert_eq!((v["name"].as_str(), v["private"].as_bool(), v["auto_init"].as_bool()), (Some("fresh"), Some(true), Some(false)), "{body}");
                (201, r#"{"name":"fresh","full_name":"ada/fresh","private":true,"empty":false,"owner":{"login":"ada"}}"#.into())
            }
            _ => (404, "{}".into()),
        }
    });
    let s = forgejo(&base);
    assert_eq!(api::whoami(&s, &token).unwrap(), "ada");
    let repos = api::list(&s, &token).unwrap();
    assert_eq!(repos.iter().map(|r| (r.full_name(), r.private, r.empty)).collect::<Vec<_>>(), [("ada/notes".to_string(), true, true), ("ada/work".to_string(), false, false)]);
    let made = api::create(&s, &token, "fresh", "desc").unwrap();
    assert_eq!((made.full_name().as_str(), made.private, made.empty), ("ada/fresh", true, true));
    // the token went in the header, never into the address
    assert_eq!(seen.join().unwrap(), ["GET /api/v1/user", "GET /api/v1/user/repos?limit=50", "POST /api/v1/user/repos"].map(String::from));
}

// spec: SYNC-055
#[test]
fn sync_055_github_lists_the_owners_repositories_and_names_clash_and_bad_tokens_in_words() {
    let (base, _) = mock_http(1, |_, path, _, _| {
        assert!(path.starts_with("/api/v3/user/repos?per_page=100"), "{path}");
        (200, r#"[{"name":"n","full_name":"ada/n","private":true,"size":0,"owner":{"login":"ada"}}]"#.into())
    });
    let gh = server(Kind::GitHub, base.trim_start_matches("http://"), "http");
    let repos = api::list(&gh, &Secret::new("t")).unwrap();
    assert!(repos[0].empty, "a GitHub repository of size 0 is empty");

    let (base, _) = mock_http(3, |m, p, _, _| match (m, p) {
        ("GET", "/api/v1/user") => (401, r#"{"message":"bad token"}"#.into()),
        ("POST", _) => (409, r#"{"message":"exists"}"#.into()),
        ("GET", "/api/v1/user/repos?limit=50") => (404, "{}".into()),
        _ => (500, r#"{"message":"boom"}"#.into()),
    });
    let s = forgejo(&base);
    let t = Secret::new("t");
    assert_eq!(api::whoami(&s, &t), Err(ApiError::Auth));
    assert_eq!(api::create(&s, &t, "dup", ""), Err(ApiError::Exists));
    assert_eq!(api::list(&s, &t), Err(ApiError::NotFound));
    assert_eq!(api::create(&s, &t, "bad name", ""), Err(ApiError::Other("A repository name may have letters, digits, - _ and . only".into())));
    for e in [ApiError::Auth, ApiError::NotFound, ApiError::Exists, ApiError::Network("x".into())] {
        assert!(!e.message().is_empty());
    }
    assert!(ApiError::Auth.message().contains("token"));
    // no answer at all
    let closed = closed_port();
    let dead = server(Kind::Forgejo, &format!("127.0.0.1:{}", closed.port), "http");
    assert!(matches!(api::whoami(&dead, &t), Err(ApiError::Network(_))));
    // a kind without an API
    assert!(matches!(api::whoami(&server(Kind::Other, "x.org", "https"), &t), Err(ApiError::Other(_))));
    assert!(api::parse_login("not json").is_err() && api::parse_login("{}").is_err() && api::parse_repos("{}").is_err());
    for ok in ["a", "my-notes", "n_1.x"] {
        assert!(api::check_repo_name(ok).is_ok(), "{ok}");
    }
    for bad in ["", ".hidden", "a b", "x.git", "über"] {
        assert!(api::check_repo_name(bad).is_err(), "{bad}");
    }
}

fn keygen_available() -> bool {
    std::process::Command::new("ssh-keygen").arg("-?").stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().is_ok()
}

// spec: SYNC-056
#[test]
fn sync_056_a_new_key_is_made_by_ssh_keygen_never_overwrites_and_shows_up_in_the_list() {
    if !keygen_available() {
        eprintln!("skipping: ssh-keygen is not installed here");
        return;
    }
    let env = Env::new("ssh-keygen");
    let home = env.folder("home");
    assert!(ssh::existing(&home).is_empty());
    let a = ssh::generate(&env.git, &home, "git.example.org:8443", "laptop").unwrap();
    assert_eq!(a.private, home.join(".ssh/shale_git.example.org_8443"));
    assert!(a.public.starts_with("ssh-ed25519 ") && a.public.ends_with("Shale on laptop"), "{}", a.public);
    assert!(a.private.is_file());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&a.private).unwrap().permissions().mode() & 0o777, 0o600, "ssh-keygen keeps the private key private");
        assert_eq!(std::fs::metadata(home.join(".ssh")).unwrap().permissions().mode() & 0o777, 0o700);
    }
    let b = ssh::generate(&env.git, &home, "git.example.org:8443", "laptop").unwrap();
    assert_eq!(b.private, home.join(".ssh/shale_git.example.org_8443_2"), "a taken name gets a number");
    assert_ne!(a.public, b.public);
    // the keys git would use are listed too
    std::fs::write(home.join(".ssh/id_ed25519"), "x").unwrap();
    std::fs::write(home.join(".ssh/id_ed25519.pub"), "ssh-ed25519 AAAA me@x\n").unwrap();
    std::fs::write(home.join(".ssh/id_rsa.pub"), "no private part but a public one: ssh-rsa AAAA\n").unwrap();
    std::fs::write(home.join(".ssh/id_ecdsa"), "x").unwrap();
    std::fs::write(home.join(".ssh/id_ecdsa.pub"), "not a key\n").unwrap();
    let names: Vec<String> = ssh::existing(&home).iter().map(ssh::Key::name).collect();
    assert_eq!(names, ["id_ed25519", "shale_git.example.org_8443", "shale_git.example.org_8443_2"]);
    assert_eq!(ssh::existing(&home)[0].public, "ssh-ed25519 AAAA me@x");
}

// spec: SYNC-056
#[test]
fn sync_056_ssh_is_run_with_the_chosen_key_only_and_never_asks_a_question() {
    assert_eq!(ssh::ssh_command(None, false).as_deref(), Some("ssh -o BatchMode=yes"));
    assert_eq!(ssh::ssh_command(None, true).as_deref(), Some("ssh -o BatchMode=yes -o StrictHostKeyChecking=accept-new"));
    assert_eq!(ssh::ssh_command(Some(Path::new("/home/u/.ssh/shale_x")), false).as_deref(), Some("ssh -i '/home/u/.ssh/shale_x' -o IdentitiesOnly=yes -o BatchMode=yes"));
    assert_eq!(ssh::ssh_command(Some(Path::new("/home/o'brien/.ssh/k")), false), None, "a path that cannot be quoted is refused");
}

// spec: SYNC-057
#[test]
fn sync_057_the_remote_is_empty_or_has_notes_and_the_words_say_what_the_first_sync_does() {
    let env = Env::new("inspect");
    let remote = env.bare();
    let v = env.vault("vault", &remote);
    assert_eq!(test_sync::inspect(&env.git, &v), Ok(Remote::Empty));
    write(&v, "Note.md", "hello\n");
    env.sync(&v);
    assert_eq!(test_sync::inspect(&env.git, &v), Ok(Remote::HasNotes));
    // a remote that is not there says why, in git's words
    env.git(&v, &["remote", "set-url", "origin", env.root.join("missing.git").to_str().unwrap()]);
    let err = test_sync::inspect(&env.git, &v).unwrap_err();
    assert_eq!(test_sync::classify(&err), Problem::NoRepository, "{err}");

    assert!(test_sync::vault_has_notes(&v));
    let empty = env.folder("empty");
    write(&empty, ".shale/settings.json", "{}");
    write(&empty, "photo.png", "x");
    assert!(!test_sync::vault_has_notes(&empty));
    write(&empty, "sub/deeper/Plan.md", "x");
    assert!(test_sync::vault_has_notes(&empty));

    let w = test_sync::what_happens;
    assert!(w(Remote::Empty, true).contains("uploaded"));
    assert!(w(Remote::Empty, false).contains("nothing to copy"));
    assert!(w(Remote::HasNotes, false).contains("downloaded"));
    let both = w(Remote::HasNotes, true);
    assert!(both.contains("merges") && both.contains("nothing is deleted") && both.contains("conflict"), "{both}");
}

// spec: SYNC-058, SYNC-079
#[test]
fn sync_058_git_s_errors_become_plain_words_for_each_case() {
    let cases: [(&str, Problem); 12] = [
        ("remote: Unauthorized\nfatal: Authentication failed for 'https://x/o/r.git/'", Problem::Sign),
        ("fatal: could not read Password for 'https://ada@x': terminal prompts disabled", Problem::Sign),
        ("fatal: unable to access 'https://x/': The requested URL returned error: 403", Problem::Sign),
        ("fatal: unable to access 'https://x/': Could not resolve host: x", Problem::NoNetwork),
        ("fatal: unable to access 'http://127.0.0.1:1/': Failed to connect to 127.0.0.1 port 1: Connection refused", Problem::NoNetwork),
        ("ssh: connect to host x port 22: Network is unreachable", Problem::NoNetwork),
        ("remote: Repository not found.\nfatal: repository 'https://x/o/r.git/' not found", Problem::NoRepository),
        ("fatal: '/srv/none.git' does not appear to be a git repository", Problem::NoRepository),
        ("git@x: Permission denied (publickey).\nfatal: Could not read from remote repository.", Problem::SshKey),
        ("Host key verification failed.", Problem::HostKey),
        ("fatal: unable to access 'https://x/': SSL certificate problem: self-signed certificate", Problem::Certificate),
        ("fatal: something nobody expected", Problem::Other("fatal: something nobody expected".into())),
    ];
    for (text, want) in cases {
        assert_eq!(test_sync::classify(text), want, "{text}");
    }
    for p in [Problem::Sign, Problem::NoNetwork, Problem::NoRepository, Problem::SshKey, Problem::HostKey, Problem::Certificate, Problem::Other("boom".into())] {
        assert!(p.words().len() > 20, "{p:?}");
    }
    assert!(Problem::Sign.words().contains("token") && Problem::NoNetwork.words().contains("internet") && Problem::Other("boom".into()).words().contains("boom"));
    let rep = crate::gitsync::Report { committed: 2, changed: vec!["A.md".into(), "B.md".into()], conflicts: vec![("B.md".into(), "B (conflict).md".into())], remote: true, pushed: true };
    let text = test_sync::worked(&rep);
    assert!(text.starts_with("It worked") && text.contains("2 note(s)") && text.contains("1 file(s) were downloaded") && text.contains("both versions") && text.contains("on the server"), "{text}");
    assert!(test_sync::worked(&crate::gitsync::Report::default()).contains("the same"));
}

// spec: SYNC-053
#[test]
fn sync_053_without_a_keyring_no_token_is_kept_anywhere_and_the_words_recommend_an_ssh_key() {
    let (base, seen) = mock_http(2, |m, p, _, _| match (m, p) {
        ("GET", "/api/v1/user") => (200, r#"{"login":"ada"}"#.into()),
        _ => (200, "[]".into()),
    });
    let s = forgejo(&base);
    crate::secrets::pretend_no_keyring(true);
    let r = super::sign_in(&s, "", &Secret::new("tok"), &crate::busy::Cancel::default());
    crate::secrets::pretend_no_keyring(false);
    let e = r.err().expect("no keyring, no sign-in");
    assert!(e.contains("no keyring") && e.contains("SSH key") && e.contains("never writes a token to a file"), "{e}");
    let key = crate::gitsync::credentials::server_key(&s.base()).unwrap();
    assert!(crate::secrets::git_token(&key, "ada").is_none());
    let _ = seen.join();
    // an empty token, and a user name for a server without an API
    assert!(super::sign_in(&s, "", &Secret::default(), &crate::busy::Cancel::default()).is_err());
    let other = server(Kind::Other, "git.example.org", "https");
    assert!(super::sign_in(&other, "", &Secret::new("t"), &crate::busy::Cancel::default()).err().is_some_and(|e| e.contains("user name")));
    assert!(super::sign_in(&other, "a b", &Secret::new("t"), &crate::busy::Cancel::default()).is_err());
    assert!(super::sign_in(&other, "ada@x", &Secret::new("t"), &crate::busy::Cancel::default()).is_err());
}

// spec: SYNC-053
#[test]
fn sync_053_a_server_without_an_api_keeps_the_token_for_the_user_the_person_named() {
    let other = server(Kind::Other, "git.example.org", "https");
    let signed = super::sign_in(&other, " ada ", &Secret::new("tok-other"), &crate::busy::Cancel::default()).unwrap();
    assert_eq!((signed.login.as_str(), signed.repos.len()), ("ada", 0));
    assert_eq!(crate::secrets::git_token("https://git.example.org", "ada").map(|s| s.expose().to_string()).as_deref(), Some("tok-other"));
}
