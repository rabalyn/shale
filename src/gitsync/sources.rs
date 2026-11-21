//! Where the managed git comes from, per platform (SYNC-029, ADR-0027): one table of recipes, each
//! pinned to a release with its size and SHA-256, so that an install checks a hash that was reviewed
//! with the code; the release a later update takes is read from GitHub's API and verified by the
//! SHA-256 that GitHub lists for its file (SYNC-027). The table is data, so tests run the Windows
//! and macOS paths on Linux with a fake one that points at a pretend server.

use serde::Deserialize;

/// The system and processor of a computer, as `std::env::consts` names them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Platform {
    pub os: String,
    pub arch: String,
}

impl Platform {
    pub fn current() -> Platform {
        Platform { os: std::env::consts::OS.to_string(), arch: std::env::consts::ARCH.to_string() }
    }

    pub fn new(os: &str, arch: &str) -> Platform {
        Platform { os: os.to_string(), arch: arch.to_string() }
    }
}

/// What a downloaded file is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Git,
    Lfs,
}

/// The release a recipe is pinned to.
#[derive(Clone, Debug)]
pub struct Pin {
    /// The tag of the release (`v3.8.1`, `v2.56.1.windows.1`).
    pub tag: String,
    /// Its version number (`3.8.1`, `2.56.1`).
    pub version: String,
    /// The download in bytes.
    pub size: u64,
    /// What it takes on disk, unpacked.
    pub disk: u64,
    pub sha256: String,
}

/// How to get one thing on one platform.
#[derive(Clone, Debug)]
pub struct Recipe {
    pub kind: Kind,
    /// "MinGit", "git-lfs".
    pub name: String,
    /// The GitHub project, `git-lfs/git-lfs`.
    pub repo: String,
    /// The file of a release; `{v}` is the version number.
    pub file: String,
    /// The folder of the managed folder the files go into ("" = the folder itself).
    pub into: String,
    /// The archive keeps everything in one top folder (`git-lfs-3.8.1/`), which is left out.
    pub strip_root: bool,
    /// The program, relative to the managed folder.
    pub program: String,
    /// The folders, relative to the managed folder, that are added to `PATH`.
    pub path: Vec<String>,
    pub pin: Pin,
}

/// One file to download: a recipe at a release.
#[derive(Clone, Debug)]
pub struct Part {
    pub recipe: Recipe,
    pub tag: String,
    pub version: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
    pub disk: u64,
}

/// What an install or update downloads.
#[derive(Clone, Debug)]
pub struct Plan {
    pub parts: Vec<Part>,
    /// `http://` addresses are accepted (a pretend server in a test).
    pub allow_http: bool,
}

impl Plan {
    pub fn download_bytes(&self) -> u64 {
        self.parts.iter().map(|p| p.size).sum()
    }

    pub fn disk_bytes(&self) -> u64 {
        self.parts.iter().map(|p| p.disk).sum()
    }

    /// "git", "git-lfs" or "git and git-lfs": what the button installs.
    pub fn what(&self) -> &'static str {
        match (self.parts.iter().any(|p| p.recipe.kind == Kind::Git), self.parts.iter().any(|p| p.recipe.kind == Kind::Lfs)) {
            (true, true) => "git and git-lfs",
            (true, false) => "git",
            _ => "git-lfs",
        }
    }

    /// "git 2.56.1, git-lfs 3.8.1".
    pub fn versions_text(&self) -> String {
        self.parts.iter().map(|p| format!("{} {}", if p.recipe.kind == Kind::Git { "git" } else { "git-lfs" }, p.version)).collect::<Vec<_>>().join(", ")
    }
}

/// The table.
#[derive(Clone, Debug)]
pub struct Sources {
    /// Where the files of a release are (`https://github.com`).
    pub download_base: String,
    /// Where the API is (`https://api.github.com`).
    pub api_base: String,
    pub allow_http: bool,
    pub table: Vec<(Platform, Vec<Recipe>)>,
}

fn lfs_recipe(file: &str, program: &str, tag: &str, version: &str, size: u64, disk: u64, sha256: &str) -> Recipe {
    Recipe {
        kind: Kind::Lfs,
        name: "git-lfs".into(),
        repo: "git-lfs/git-lfs".into(),
        file: file.into(),
        into: "lfs".into(),
        strip_root: true,
        program: format!("lfs/{program}"),
        path: vec!["lfs".into()],
        pin: Pin { tag: tag.into(), version: version.into(), size, disk, sha256: sha256.into() },
    }
}

fn mingit_recipe(file: &str, size: u64, sha256: &str) -> Recipe {
    Recipe {
        kind: Kind::Git,
        name: "MinGit".into(),
        repo: "git-for-windows/git".into(),
        file: file.into(),
        into: String::new(),
        strip_root: false,
        program: "cmd/git.exe".into(),
        path: vec!["cmd".into()],
        pin: Pin { tag: "v2.56.1.windows.1".into(), version: "2.56.1".into(), size, disk: 96_243_738, sha256: sha256.into() },
    }
}

impl Sources {
    /// The real table (ADR-0027, checked 2026-11-21): MinGit 2.56.1 and git-lfs 3.8.1.
    pub fn real() -> Sources {
        let lfs = |file: &str, program: &str, size: u64, disk: u64, sha: &str| lfs_recipe(file, program, "v3.8.1", "3.8.1", size, disk, sha);
        let table = vec![
            (
                Platform::new("windows", "x86_64"),
                vec![
                    mingit_recipe("MinGit-{v}-64-bit.zip", 39_849_768, "4a92850b6e917da4fd010669b614b55dccf5b0ec333a534cc4be4700dfefdafd"),
                    lfs("git-lfs-windows-amd64-v{v}.zip", "git-lfs.exe", 5_870_805, 15_320_675, "5bb771c12b3dacc535e912d0270a010c209d4da2e63819c5918e3547c2defed9"),
                ],
            ),
            (
                Platform::new("windows", "aarch64"),
                vec![
                    mingit_recipe("MinGit-{v}-arm64.zip", 37_983_254, "96571e442e90523a90e5fd8661b4c4e43bc192910f3c6db938816e6955b4cc8c"),
                    lfs("git-lfs-windows-arm64-v{v}.zip", "git-lfs.exe", 5_270_378, 14_500_000, "7e70990e339efab3e793affb919355b548c9a41c5b6bb52d90267bcb84ba4784"),
                ],
            ),
            (Platform::new("macos", "x86_64"), vec![lfs("git-lfs-darwin-amd64-v{v}.zip", "git-lfs", 6_202_683, 15_500_000, "4292b3518e717e53c20a6c2c0696e2069a21e97500ad14b80b06bd52de8f3b72")]),
            (Platform::new("macos", "aarch64"), vec![lfs("git-lfs-darwin-arm64-v{v}.zip", "git-lfs", 5_559_976, 14_883_222, "0ba10e897e0496412cd86eacaed4ed554c97ddc29bc5c709d28faa5eaaa2222c")]),
            (Platform::new("linux", "x86_64"), vec![lfs("git-lfs-linux-amd64-v{v}.tar.gz", "git-lfs", 5_917_265, 15_553_798, "e72cf723a4a660438862282e8aecf3885fc8123e88bfcad534cf6b941a354b75")]),
            (Platform::new("linux", "aarch64"), vec![lfs("git-lfs-linux-arm64-v{v}.tar.gz", "git-lfs", 5_350_198, 15_000_000, "67895dd77b63329796bc5efdd47b57f218e1f7b98717ebaf936a8238dbcaec0f")]),
        ];
        Sources { download_base: "https://github.com".into(), api_base: "https://api.github.com".into(), allow_http: false, table }
    }

    /// The recipes of a platform; none: Shale has no download for it.
    pub fn recipes(&self, platform: &Platform) -> Option<&[Recipe]> {
        self.table.iter().find(|(p, _)| p == platform).map(|(_, r)| r.as_slice())
    }

    /// The file of a recipe at its pinned release.
    pub fn pinned(&self, r: &Recipe) -> Part {
        let file = r.file.replace("{v}", &r.pin.version);
        Part {
            recipe: r.clone(),
            tag: r.pin.tag.clone(),
            version: r.pin.version.clone(),
            url: format!("{}/{}/releases/download/{}/{file}", self.download_base, r.repo, r.pin.tag),
            sha256: r.pin.sha256.clone(),
            size: r.pin.size,
            disk: r.pin.disk,
        }
    }

    /// The pinned install of these recipes.
    pub fn pinned_plan(&self, recipes: &[&Recipe]) -> Plan {
        Plan { parts: recipes.iter().map(|r| self.pinned(r)).collect(), allow_http: self.allow_http }
    }

    /// The address of the newest release of a project.
    pub fn latest_url(&self, repo: &str) -> String {
        format!("{}/repos/{repo}/releases/latest", self.api_base)
    }

    /// The file of a recipe at the release the API answered with (`json`): its address (only one of
    /// the project's release downloads), size and the SHA-256 GitHub lists for it. An asset without
    /// a checksum is refused (SYNC-027).
    pub fn latest(&self, r: &Recipe, json: &str) -> Result<Part, String> {
        #[derive(Deserialize)]
        struct Asset {
            name: String,
            size: u64,
            digest: Option<String>,
            browser_download_url: String,
        }
        #[derive(Deserialize)]
        struct Release {
            tag_name: String,
            assets: Vec<Asset>,
        }
        let rel: Release = serde_json::from_str(json).map_err(|e| format!("the answer for {} is not a release ({e})", r.repo))?;
        let version = version_of(&rel.tag_name).ok_or_else(|| format!("{:?} is no version of {}", rel.tag_name, r.repo))?;
        let file = r.file.replace("{v}", &version);
        let asset = rel.assets.iter().find(|a| a.name == file).ok_or_else(|| format!("the release {} of {} has no {file}", rel.tag_name, r.repo))?;
        let sha = asset.digest.as_deref().and_then(|d| d.strip_prefix("sha256:")).map(str::to_ascii_lowercase).filter(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()));
        let sha256 = sha.ok_or_else(|| format!("the release {} of {} lists no SHA-256 for {file}: refused", rel.tag_name, r.repo))?;
        let prefix = format!("{}/{}/releases/download/", self.download_base, r.repo);
        if !asset.browser_download_url.starts_with(&prefix) {
            return Err(format!("{} is not a release download of {}: refused", asset.browser_download_url, r.repo));
        }
        Ok(Part { recipe: r.clone(), tag: rel.tag_name, version, url: asset.browser_download_url.clone(), sha256, size: asset.size, disk: r.pin.disk })
    }
}

/// `2.56.1` for the tag `v2.56.1.windows.1`, `3.8.1` for `v3.8.1`.
pub fn version_of(tag: &str) -> Option<String> {
    let t = tag.strip_prefix('v').unwrap_or(tag);
    let v: String = t.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    let v = v.trim_end_matches('.');
    (!v.is_empty() && v.starts_with(|c: char| c.is_ascii_digit())).then(|| v.to_string())
}

/// The numbers of a tag, `v2.56.1.windows.1` as 2, 56, 1, 1.
fn numbers(tag: &str) -> Vec<u64> {
    tag.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty()).filter_map(|s| s.parse().ok()).collect()
}

/// Whether `latest` is a newer release than `installed`, by their numbers.
pub fn is_newer(installed: &str, latest: &str) -> bool {
    let (a, b) = (numbers(installed), numbers(latest));
    let n = a.len().max(b.len());
    let at = |v: &[u64], i: usize| v.get(i).copied().unwrap_or(0);
    (0..n).map(|i| at(&b, i).cmp(&at(&a, i))).find(|o| o.is_ne()).is_some_and(|o| o.is_gt())
}

// ---------------------------------------------------------------- what to offer

/// What helps when no git can be downloaded for this system.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GitHelp {
    /// macOS: `xcode-select --install`, Apple's dialog.
    Xcode,
    /// Linux: the command of the distribution's package manager.
    Package(PackageHelp),
    /// Linux whose distribution is not known.
    Unknown,
}

/// The install command of a distribution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageHelp {
    /// "Debian", the name from `os-release`.
    pub distro: String,
    pub command: String,
}

/// What the page offers (SYNC-025, SYNC-029).
#[derive(Clone, Debug)]
pub struct Offer {
    /// The download the button starts.
    pub download: Option<Plan>,
    /// What to do instead when git is missing and Shale cannot download it.
    pub help: Option<GitHelp>,
    /// Why nothing can be offered for this system.
    pub unsupported: Option<String>,
}

/// What is there already.
#[derive(Clone, Copy, Debug)]
pub struct Have {
    pub git: bool,
    pub lfs: bool,
}

impl Sources {
    /// What to offer on `platform` when `have` is there. Windows downloads MinGit with git-lfs when
    /// there is no git, git-lfs alone when only that is missing; macOS and Linux have no portable git
    /// (ADR-0027): Apple's dialog or the distribution's command when there is no git, and git-lfs when
    /// only that is missing.
    pub fn offer(&self, platform: &Platform, have: Have, os_release: Option<&str>) -> Offer {
        let Some(recipes) = self.recipes(platform) else {
            return Offer { download: None, help: None, unsupported: Some(format!("Shale has no download of git for {} on {}.", platform.os, platform.arch)) };
        };
        let can_download_git = recipes.iter().any(|r| r.kind == Kind::Git);
        let pick = |kinds: &[Kind]| recipes.iter().filter(|r| kinds.contains(&r.kind)).collect::<Vec<_>>();
        if !have.git {
            if can_download_git {
                return Offer { download: Some(self.pinned_plan(&pick(&[Kind::Git, Kind::Lfs]))), help: None, unsupported: None };
            }
            let help = match platform.os.as_str() {
                "macos" => GitHelp::Xcode,
                _ => package_help(os_release.unwrap_or("")).map_or(GitHelp::Unknown, GitHelp::Package),
            };
            return Offer { download: None, help: Some(help), unsupported: None };
        }
        let download = (!have.lfs).then(|| self.pinned_plan(&pick(&[Kind::Lfs]))).filter(|p| !p.parts.is_empty());
        Offer { download, help: None, unsupported: None }
    }
}

/// The value of a key of `os-release` (`ID=debian`, `PRETTY_NAME="Debian GNU/Linux 12"`).
fn os_release_value(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|l| l.trim().strip_prefix(key)?.strip_prefix('=')).map(|v| v.trim().trim_matches(|c| c == '"' || c == '\'').to_string()).filter(|v| !v.is_empty())
}

/// The command that installs git and git-lfs on the distribution `os-release` describes: by `ID`,
/// then by each word of `ID_LIKE`.
pub fn package_help(os_release: &str) -> Option<PackageHelp> {
    let command_of = |id: &str| -> Option<&'static str> {
        Some(match id {
            "debian" | "ubuntu" | "linuxmint" | "pop" | "raspbian" | "elementary" | "kali" | "zorin" => "sudo apt install git git-lfs",
            "fedora" | "rhel" | "centos" | "rocky" | "almalinux" | "ol" | "amzn" => "sudo dnf install git git-lfs",
            "arch" | "manjaro" | "endeavouros" | "cachyos" | "garuda" | "artix" => "sudo pacman -S git git-lfs",
            "opensuse" | "opensuse-leap" | "opensuse-tumbleweed" | "sles" | "sled" | "suse" => "sudo zypper install git git-lfs",
            "alpine" | "postmarketos" => "sudo apk add git git-lfs",
            _ => return None,
        })
    };
    let id = os_release_value(os_release, "ID").unwrap_or_default().to_ascii_lowercase();
    let like = os_release_value(os_release, "ID_LIKE").unwrap_or_default().to_ascii_lowercase();
    let command = std::iter::once(id.as_str()).chain(like.split_whitespace()).find_map(command_of)?;
    let distro = os_release_value(os_release, "NAME").unwrap_or_else(|| id.clone());
    Some(PackageHelp { distro, command: command.to_string() })
}
