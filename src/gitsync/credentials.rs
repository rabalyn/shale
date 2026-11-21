//! How git gets a token without Shale ever putting it in a file, a URL or a command line
//! (SYNC-053, SYNC-054, ADR-0029).
//!
//! Git asks `GIT_ASKPASS` for a password. For a `Git` that Shale runs, `GIT_ASKPASS` is the Shale
//! executable and the environment variable [`ASKPASS_ENV`] is `1`: started like that with one
//! argument (git's prompt), `main` calls [`run_askpass`] before it opens anything, which reads the
//! keyring entry of the server and user the prompt names, prints the token and exits. A prompt
//! it has no token for gets no answer and exit code 1, so git fails at once, as it does without it.

use super::Git;
use crate::secret::Secret;
use std::path::Path;

/// Marks a started Shale as git's askpass helper.
pub const ASKPASS_ENV: &str = "SHALE_GIT_ASKPASS";

/// What git asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prompt {
    /// `Password` or `Username`.
    pub password: bool,
    /// `https://host[:port]`, as the keyring entry names the server.
    pub server: String,
    /// The user name the prompt names (`https://user@host`).
    pub user: Option<String>,
}

/// The key of a server in the keyring: `scheme://host[:port]` in lower case, the default port left
/// out. None for an address that is not `http(s)://host…`.
pub fn server_key(url: &str) -> Option<String> {
    let (scheme, rest) = url.trim().split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "https" && scheme != "http" {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host_port = authority.rsplit_once('@').map_or(authority, |(_, h)| h).to_ascii_lowercase();
    let default = if scheme == "https" { ":443" } else { ":80" };
    let host_port = host_port.strip_suffix(default).unwrap_or(&host_port);
    (!host_port.is_empty()).then(|| format!("{scheme}://{host_port}"))
}

/// The user name inside an address (`https://user@host/…`).
pub fn user_of(url: &str) -> Option<String> {
    let rest = url.trim().split_once("://")?.1;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let (userinfo, _) = authority.rsplit_once('@')?;
    let user = userinfo.split(':').next().unwrap_or("");
    (!user.is_empty()).then(|| user.to_string())
}

/// Read git's prompt: `Password for 'https://user@host': ` or `Username for 'https://host': `.
pub fn parse_prompt(prompt: &str) -> Option<Prompt> {
    let (password, rest) = if let Some(r) = prompt.strip_prefix("Password for '") {
        (true, r)
    } else {
        (false, prompt.strip_prefix("Username for '")?)
    };
    let url = rest.split('\'').next()?;
    Some(Prompt { password, server: server_key(url)?, user: user_of(url) })
}

/// What the helper prints for a prompt: the token for a password prompt that names a user and a
/// server with a stored token, nothing else (`lookup` reads the keyring).
pub fn answer(prompt: &str, lookup: impl Fn(&str, &str) -> Option<Secret>) -> Option<Secret> {
    let p = parse_prompt(prompt)?;
    if !p.password {
        return None;
    }
    lookup(&p.server, p.user.as_deref()?)
}

/// Does this started Shale act as git's askpass helper? `args` are the arguments after the program.
pub fn askpass_requested(env_value: Option<&std::ffi::OsStr>, args: &[String]) -> bool {
    env_value.is_some_and(|v| v == "1") && args.len() == 1 && (args[0].starts_with("Password for ") || args[0].starts_with("Username for "))
}

/// The helper: print the token (and a newline) and return 0, or print nothing and return 1.
pub fn run_askpass(prompt: &str) -> i32 {
    use std::io::Write;
    match answer(prompt, crate::secrets::git_token) {
        Some(token) => {
            let mut out = std::io::stdout().lock();
            if out.write_all(token.expose().as_bytes()).and_then(|()| out.write_all(b"\n")).and_then(|()| out.flush()).is_ok() {
                0
            } else {
                1
            }
        }
        None => 1,
    }
}

/// Make `git` ask this executable for passwords (SYNC-054): `GIT_ASKPASS`, the marker, and no
/// interactive window of the Git Credential Manager. The person's own credential helpers stay as
/// they were and are asked first, so an existing setup keeps working.
pub fn attach(git: &mut Git, exe: &Path) {
    let set = |git: &mut Git, k: &str, v: std::ffi::OsString| {
        git.env.retain(|(key, _)| key != k);
        git.env.push((k.into(), v));
    };
    set(git, "GIT_ASKPASS", exe.as_os_str().to_os_string());
    set(git, ASKPASS_ENV, "1".into());
    set(git, "GCM_INTERACTIVE", "never".into());
}
