//! List and create repositories through a provider's API (SYNC-055): Forgejo and Gitea
//! (`/api/v1/user/repos`) and GitHub (`/user/repos`). The token goes in an `Authorization` header
//! of curl's configuration on standard input (SEC-002, SEC-017), only to the server the person
//! named; the HTTP client is the one of the CalDAV code. Every call is slow: run it in a job.

use super::provider::Server;
use crate::calendar::dav::{self, Account};
use crate::secret::Secret;
use zeroize::Zeroizing;

/// Why a call failed, told apart so the wizard can say what to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ApiError {
    /// The server did not accept the token.
    Auth,
    /// No such address (the wrong kind of server, or a wrong address).
    NotFound,
    /// A repository of that name exists already.
    Exists,
    /// No answer from the server.
    Network(String),
    Other(String),
}

impl ApiError {
    /// In plain words.
    pub fn message(&self) -> String {
        match self {
            ApiError::Auth => "The server did not accept the token. It may be wrong, have expired, or lack the right to use repositories.".into(),
            ApiError::NotFound => "The server does not know that address. Check the address, and that you picked the right kind of server.".into(),
            ApiError::Exists => "You already have a repository with that name. Pick it from the list, or choose another name.".into(),
            ApiError::Network(e) => format!("Could not reach the server: {e}"),
            ApiError::Other(e) => e.clone(),
        }
    }
}

/// A repository of the person's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepoInfo {
    pub owner: String,
    pub name: String,
    pub private: bool,
    /// Nothing in it yet (where the server says so).
    pub empty: bool,
}

impl RepoInfo {
    pub fn full_name(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }
}

fn call(server: &Server, method: &str, path: &str, token: &Secret, body: &str) -> Result<(u16, String), ApiError> {
    let api = server.api().ok_or_else(|| ApiError::Other("This kind of server has no list of repositories here".into()))?;
    let url = format!("{api}{path}");
    let mut auth = Zeroizing::new(String::with_capacity(token.len() + 24));
    auth.push_str("Authorization: token ");
    auth.push_str(token.expose());
    let headers = [auth.as_str(), "Accept: application/json", "Content-Type: application/json", "User-Agent: Shale"];
    let account = Account { url: String::new(), username: String::new(), password: Secret::default() };
    let r = dav::request_with(method, &url, &headers, body, &account).map_err(ApiError::Network)?;
    Ok((r.status, r.body))
}

/// The message a Forgejo, Gitea or GitHub error answer carries.
fn message_of(body: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    v.get("message")?.as_str().map(str::to_string)
}

/// Turn a status into an error, or hand the body back.
fn checked(status: u16, body: String) -> Result<String, ApiError> {
    match status {
        200..=299 => Ok(body),
        401 | 403 => Err(ApiError::Auth),
        404 => Err(ApiError::NotFound),
        409 | 422 => Err(ApiError::Exists),
        s => Err(ApiError::Other(format!("The server answered {s}{}", message_of(&body).map(|m| format!(": {m}")).unwrap_or_default()))),
    }
}

/// The login of the token's owner (also the check that the token works).
pub fn whoami(server: &Server, token: &Secret) -> Result<String, ApiError> {
    let (status, body) = call(server, "GET", "/user", token, "")?;
    parse_login(&checked(status, body)?)
}

pub fn parse_login(body: &str) -> Result<String, ApiError> {
    let v: serde_json::Value = serde_json::from_str(body).map_err(|_| ApiError::Other("The server's answer was not what Shale expected".into()))?;
    v.get("login").and_then(|l| l.as_str()).filter(|l| !l.is_empty()).map(str::to_string).ok_or_else(|| ApiError::Other("The server did not say who the token belongs to".into()))
}

/// The person's own repositories, the most recently changed first (one page).
pub fn list(server: &Server, token: &Secret) -> Result<Vec<RepoInfo>, ApiError> {
    let path = if server.api().is_some_and(|a| a.contains("/api/v1")) { "/user/repos?limit=50" } else { "/user/repos?per_page=100&affiliation=owner&sort=updated" };
    let (status, body) = call(server, "GET", path, token, "")?;
    parse_repos(&checked(status, body)?)
}

pub fn parse_repos(body: &str) -> Result<Vec<RepoInfo>, ApiError> {
    let v: serde_json::Value = serde_json::from_str(body).map_err(|_| ApiError::Other("The server's list of repositories was not what Shale expected".into()))?;
    let items = v.as_array().ok_or_else(|| ApiError::Other("The server's list of repositories was not what Shale expected".into()))?;
    Ok(items.iter().filter_map(repo_of).collect())
}

fn repo_of(v: &serde_json::Value) -> Option<RepoInfo> {
    let name = v.get("name")?.as_str()?.to_string();
    let owner = v.get("owner").and_then(|o| o.get("login")).and_then(|l| l.as_str()).map(str::to_string).or_else(|| v.get("full_name")?.as_str()?.split('/').next().map(str::to_string))?;
    let empty = v.get("empty").and_then(|e| e.as_bool()).unwrap_or_else(|| v.get("size").and_then(|s| s.as_u64()) == Some(0));
    Some(RepoInfo { owner, name, private: v.get("private").and_then(|p| p.as_bool()).unwrap_or(true), empty })
}

/// A repository name the servers accept: letters, digits, `-`, `_` and `.`, not starting with a dot.
pub fn check_repo_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("Give the repository a name".into());
    }
    if name.len() > 100 || name.starts_with('.') || name.ends_with(".git") || !name.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)) {
        return Err("A repository name may have letters, digits, - _ and . only".into());
    }
    Ok(())
}

/// Create a private repository, empty.
pub fn create(server: &Server, token: &Secret, name: &str, description: &str) -> Result<RepoInfo, ApiError> {
    check_repo_name(name).map_err(ApiError::Other)?;
    let body = serde_json::json!({ "name": name, "description": description, "private": true, "auto_init": false }).to_string();
    let (status, answer) = call(server, "POST", "/user/repos", token, &body)?;
    let v: serde_json::Value = serde_json::from_str(&checked(status, answer)?).map_err(|_| ApiError::Other("The server's answer was not what Shale expected".into()))?;
    let mut info = repo_of(&v).ok_or_else(|| ApiError::Other("The server did not describe the new repository".into()))?;
    info.empty = true;
    Ok(info)
}
