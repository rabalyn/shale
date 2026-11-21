//! The git servers the wizard knows (SYNC-051): what a host name or address means, and the pages
//! of a provider where the person makes a token, adds an SSH key or creates a repository.

/// What kind of server it is, which decides how the wizard talks to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    GitHub,
    GitLab,
    /// Forgejo and Gitea share one API (`/api/v1`).
    Forgejo,
    /// Any git server: the person pastes the address of a repository.
    Other,
}

impl Kind {
    /// What the person picks by.
    pub fn label(self) -> &'static str {
        match self {
            Kind::GitHub => "GitHub",
            Kind::GitLab => "GitLab",
            Kind::Forgejo => "Forgejo or Gitea",
            Kind::Other => "Another git server",
        }
    }
}

/// The servers with a button: (label, kind, host).
pub const PRESETS: [(&str, Kind, &str); 3] = [("GitHub", Kind::GitHub, "github.com"), ("GitLab", Kind::GitLab, "gitlab.com"), ("Codeberg", Kind::Forgejo, "codeberg.org")];

/// A server: where it is and what it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Server {
    pub kind: Kind,
    /// `https` (or `http` for a server on this computer).
    pub scheme: String,
    /// `host` or `host:port`, lower case.
    pub host: String,
}

impl Server {
    /// `https://host[:port]`.
    pub fn base(&self) -> String {
        format!("{}://{}", self.scheme, self.host)
    }

    /// The root of the API: GitHub's own host for github.com, `/api/v3` of an Enterprise server,
    /// `/api/v1` of Forgejo and Gitea; GitLab and others have none the wizard uses.
    pub fn api(&self) -> Option<String> {
        match self.kind {
            Kind::GitHub if self.host == "github.com" => Some("https://api.github.com".into()),
            Kind::GitHub => Some(format!("{}/api/v3", self.base())),
            Kind::Forgejo => Some(format!("{}/api/v1", self.base())),
            Kind::GitLab | Kind::Other => None,
        }
    }

    /// The wizard can list and create repositories here.
    pub fn has_api(&self) -> bool {
        self.api().is_some()
    }

    /// The page where the person makes a personal access token.
    pub fn token_page(&self) -> Option<String> {
        match self.kind {
            Kind::GitHub => Some(format!("{}/settings/tokens/new?scopes=repo&description=Shale", self.base())),
            Kind::GitLab => Some(format!("{}/-/user_settings/personal_access_tokens?name=Shale&scopes=read_repository,write_repository", self.base())),
            Kind::Forgejo => Some(format!("{}/user/settings/applications", self.base())),
            Kind::Other => None,
        }
    }

    /// The page where the person adds an SSH key.
    pub fn ssh_keys_page(&self) -> Option<String> {
        match self.kind {
            Kind::GitHub => Some(format!("{}/settings/ssh/new", self.base())),
            Kind::GitLab => Some(format!("{}/-/user_settings/ssh_keys", self.base())),
            Kind::Forgejo => Some(format!("{}/user/settings/keys", self.base())),
            Kind::Other => None,
        }
    }

    /// The page where the person makes a repository when the wizard cannot.
    pub fn new_repo_page(&self) -> Option<String> {
        match self.kind {
            Kind::GitHub => Some(format!("{}/new", self.base())),
            Kind::GitLab => Some(format!("{}/projects/new", self.base())),
            Kind::Forgejo => Some(format!("{}/repo/create", self.base())),
            Kind::Other => None,
        }
    }

    /// `https://user@host/owner/repo.git`: the user name in the address (SEC-010), the token never.
    pub fn https_remote(&self, user: &str, owner: &str, repo: &str) -> String {
        format!("{}://{user}@{}/{owner}/{repo}.git", self.scheme, self.host)
    }

    /// `git@host:owner/repo.git`, the SSH address (the host without a port).
    pub fn ssh_remote(&self, owner: &str, repo: &str) -> String {
        let host = self.host.split(':').next().unwrap_or(&self.host);
        format!("git@{host}:{owner}/{repo}.git")
    }
}

/// What a typed address says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Parsed {
    pub server: Server,
    /// The owner and repository, when the address names one.
    pub repo: Option<(String, String)>,
    /// It was an SSH address (`git@host:…`, `ssh://…`).
    pub ssh: bool,
}

fn kind_of(host: &str, hint: Kind) -> Kind {
    match host {
        "github.com" => Kind::GitHub,
        "gitlab.com" => Kind::GitLab,
        "codeberg.org" => Kind::Forgejo,
        _ => hint,
    }
}

/// `owner/repo(.git)` from the path of an address; the owner may be a group path for GitLab.
fn repo_of(path: &str) -> Option<(String, String)> {
    let path = path.trim_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let (owner, repo) = path.rsplit_once('/')?;
    (!owner.is_empty() && !repo.is_empty()).then(|| (owner.to_string(), repo.to_string()))
}

/// Read what the person typed: a host name (`git.example.org`), an `https://` address of a server or
/// of a repository, or an SSH address. `hint` is the kind to assume for a host that is not known.
pub fn parse(text: &str, hint: Kind) -> Result<Parsed, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("Enter the address of the server".into());
    }
    // git@host:owner/repo.git and ssh://git@host/owner/repo.git
    let ssh_rest = text.strip_prefix("ssh://").map(|r| r.split_once('@').map_or(r, |(_, h)| h).replacen('/', ":", 1)).or_else(|| {
        let (user_host, path) = text.split_once(':')?;
        (!text.contains("://") && user_host.contains('@')).then(|| format!("{}:{path}", user_host.split_once('@').map_or(user_host, |(_, h)| h)))
    });
    if let Some(rest) = ssh_rest {
        let (host, path) = rest.split_once(':').unwrap_or((rest.as_str(), ""));
        let host = host.to_ascii_lowercase();
        if host.is_empty() || !host.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-') {
            return Err("That does not look like an SSH address (git@host:owner/repo.git)".into());
        }
        let server = Server { kind: kind_of(&host, hint), scheme: "https".into(), host };
        return Ok(Parsed { server, repo: repo_of(path), ssh: true });
    }
    let url = if text.contains("://") { text.to_string() } else { format!("https://{text}") };
    let (scheme, rest) = url.split_once("://").unwrap_or(("https", &url));
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, path) = rest.split_at(end);
    // a user name in the address is not part of the server; a password is refused
    if authority.rsplit_once('@').is_some_and(|(info, _)| info.contains(':')) {
        return Err("The address has a password in it. Remove it: Shale asks for a token and keeps it in your keyring instead.".into());
    }
    let authority = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let plain = format!("{scheme}://{authority}");
    super::super::check_url(&plain)?;
    let origin = crate::calendar::dav::Origin::parse(&plain).map_err(|_| "That does not look like a server address".to_string())?;
    let port = match (origin.scheme.as_str(), origin.port) {
        ("https", 443) | ("http", 80) => String::new(),
        (_, p) => format!(":{p}"),
    };
    let host = format!("{}{port}", origin.host);
    let bare = origin.host.clone();
    let server = Server { kind: kind_of(&bare, hint), scheme: origin.scheme, host };
    Ok(Parsed { server, repo: repo_of(path.split(['?', '#']).next().unwrap_or("")), ssh: false })
}
