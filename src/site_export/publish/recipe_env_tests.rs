//! The Forgejo Actions workflow of the user guide's server recipe (SITE-049), run for real against
//! the Forgejo test stack: a runner (`tests/forgejo-env/runner.yaml`, the `host` label, so no nested
//! containers) takes the workflow of the note itself, checks out a published branch with its LFS
//! files, downloads Hugo and Pagefind as the note says, builds and swaps the folder in. Like the
//! publish tests of `env_tests.rs`: `cargo test -- --ignored forgejo_env` with the stack up; the
//! first run builds the runner image, and the workflow downloads Hugo and Pagefind from GitHub.
#![cfg(target_os = "linux")]

use super::env_tests::{go, run_ok, tip, vault, with_lfs};
use super::*;
use crate::calendar::dav::{self, Account};
use crate::gitsync::wizard::forgejo_env_tests::{make_token, server, unique, Stack, PORT};
use crate::secret::Secret;

/// The Forgejo test stack's script (`up`, the Actions runner, the logs of its jobs).
fn stack_script(args: &[&str]) -> std::process::Output {
    std::process::Command::new(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/forgejo-env/forgejo-env.sh")).args(args).output().unwrap()
}

/// The Actions runner of the stack, whose `/srv/www` is a folder of the test; gone when dropped.
struct Runner;

impl Runner {
    fn start(www: &Path) -> Runner {
        let o = stack_script(&["runner-up", &www.to_string_lossy()]);
        assert!(o.status.success(), "the Actions runner did not start:\n{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
        Runner
    }
}

impl Drop for Runner {
    fn drop(&mut self) {
        stack_script(&["runner-down"]);
    }
}

/// One call of Forgejo's API as the administrator, with the token.
fn api(token: &Secret, method: &str, path: &str, body: &serde_json::Value) -> dav::Response {
    let account = Account { url: String::new(), username: "shale".into(), password: token.clone() };
    let body = if body.is_null() { String::new() } else { body.to_string() };
    dav::request_with(method, &format!("http://127.0.0.1:{PORT}/api/v1{path}"), &["Content-Type: application/json", "Accept: application/json"], &body, &account).unwrap()
}

/// The runs of the workflows of a repository: (event, status).
fn runs(token: &Secret, repo: &str) -> Vec<(String, String)> {
    let r = api(token, "GET", &format!("/repos/shale/{repo}/actions/tasks?limit=50"), &serde_json::Value::Null);
    assert_eq!(r.status, 200, "{}", r.body);
    let v: serde_json::Value = serde_json::from_str(&r.body).unwrap();
    v["workflow_runs"].as_array().unwrap().iter().map(|r| (r["event"].as_str().unwrap_or("").to_string(), r["status"].as_str().unwrap_or("").to_string())).collect()
}

/// Press *Run workflow* and wait until the workflow has run that many times for it and no run is
/// left working; the log of the jobs is in the panic of a run that did not succeed.
fn dispatch_and_wait(token: &Secret, builder: &str, dispatches: usize) {
    let r = api(token, "POST", &format!("/repos/shale/{builder}/actions/workflows/site.yml/dispatches"), &serde_json::json!({"ref": "main"}));
    assert_eq!(r.status, 204, "{}", r.body);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(480);
    loop {
        let all = runs(token, builder);
        let working = all.iter().any(|(_, s)| matches!(s.as_str(), "running" | "waiting" | "queued" | "blocked"));
        if !working && all.iter().filter(|(e, _)| e == "workflow_dispatch").count() >= dispatches {
            if all.iter().any(|(_, s)| s != "success") {
                let log = stack_script(&["job-log", &format!("shale/{builder}")]);
                panic!("a run of the workflow did not succeed: {all:?}\n{}{}", String::from_utf8_lossy(&log.stdout), String::from_utf8_lossy(&log.stderr));
            }
            return;
        }
        assert!(std::time::Instant::now() < deadline, "the workflow did not finish in time: {all:?}");
        std::thread::sleep(std::time::Duration::from_secs(3));
    }
}

/// Deletes a builder repository when dropped, so that its schedule (every five minutes) does not go on.
struct DropBuilder {
    token: Secret,
    repo: String,
}

impl Drop for DropBuilder {
    fn drop(&mut self) {
        api(&self.token, "DELETE", &format!("/repos/shale/{}", self.repo), &serde_json::Value::Null);
    }
}

/// Deletes the builder repositories that earlier runs left behind (a run that was stopped).
fn drop_builders(token: &Secret) {
    let r = api(token, "GET", "/repos/search?q=site-builder-&limit=50", &serde_json::Value::Null);
    let v: serde_json::Value = serde_json::from_str(&r.body).unwrap_or_default();
    for name in v["data"].as_array().into_iter().flatten().filter_map(|r| r["name"].as_str()).filter(|n| n.starts_with("site-builder-")) {
        api(token, "DELETE", &format!("/repos/shale/{name}"), &serde_json::Value::Null);
    }
}

/// The folder the link `notes` points at: the link says `/srv/www/…` as the runner sees it, which is `www` here.
fn served(www: &Path) -> PathBuf {
    let target = std::fs::read_link(www.join("notes")).expect("the served folder is a link");
    www.join(target.strip_prefix("/srv/www").unwrap_or_else(|_| panic!("the link points to {}", target.display())))
}

/// The names of the builds below `notes-releases`.
fn releases(www: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(www.join("notes-releases")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| !n.starts_with('.')).collect();
    names.sort();
    names
}

// spec: SITE-049, SITE-069
#[test]
#[ignore = "needs the Forgejo test stack: tests/forgejo-env/forgejo-env.sh up"]
fn forgejo_env_the_workflow_of_the_note_builds_the_published_branch_with_its_lfs_files_in_a_runner_and_swaps_the_site_in() {
    let st = Stack::new("recipe-workflow");
    let git = with_lfs(&st);
    if git.lfs_version().is_none() {
        eprintln!("skipping: git-lfs is not installed here (CI installs it)");
        return;
    }
    let token = make_token();
    st.keep("shale", token.expose());
    // the builder repositories of earlier runs look at repositories that are gone, and their schedules would use this runner
    drop_builders(&token);
    let www = st.root.join("www");
    let _runner = Runner::start(&www);
    let (site, builder) = (unique("site"), unique("site-builder"));
    crate::gitsync::wizard::api::create(&server(), &token, &site, "").unwrap();

    // a vault with a note and a song is published; the song is an LFS file of the server
    let url = server().https_remote("shale", "shale", &site);
    let f = vault("recipe-workflow", &url, &[("Songs.md", "Zebrafish listen ![[tune.mp3]]\n")]);
    let song = format!("not an mp3 but some bytes of one, {}", unique("song")).into_bytes();
    f.bytes("tune.mp3", &song);
    let base = f.root.join("data");
    let first = go(&git, &f, &base, false).unwrap();
    assert_eq!(first.summary.media, 1);

    // the builder repository: the note's workflow for these repositories, and the token that reads the site repository
    crate::gitsync::wizard::api::create(&server(), &token, &builder, "").unwrap();
    let _gone = DropBuilder { token: token.clone(), repo: builder.clone() };
    let workflow = super::recipe_tests::fenced("yaml");
    assert!(workflow.contains("repository: me/site"), "the workflow of the note names the repository to change");
    let dir = st.root.join("builder");
    std::fs::create_dir_all(dir.join(".forgejo/workflows")).unwrap();
    std::fs::write(dir.join(".forgejo/workflows/site.yml"), workflow.replace("me/site", &format!("shale/{site}"))).unwrap();
    run_ok(&git, &dir, &["init", "--quiet", "--initial-branch", "main"]);
    run_ok(&git, &dir, &["add", "-A"]);
    run_ok(&git, &dir, &["-c", "user.name=test", "-c", "user.email=test@example.test", "commit", "--quiet", "-m", "the workflow of the note"]);
    let r = api(&token, "PUT", &format!("/repos/shale/{builder}/actions/secrets/SITE_READ_TOKEN"), &serde_json::json!({"data": token.expose()}));
    assert!(matches!(r.status, 201 | 204), "{}", r.body);
    run_ok(&git, &dir, &["push", "--quiet", &server().https_remote("shale", "shale", &builder), "HEAD:refs/heads/main"]);

    // press *Run workflow*: the runner checks out with LFS, builds with Hugo and Pagefind and swaps the folder in
    dispatch_and_wait(&token, &builder, 1);
    assert!(www.join("notes").is_symlink(), "the served folder is a link");
    let built = served(&www);
    assert_eq!(built.parent().unwrap(), www.join("notes-releases"));
    let page = std::fs::read_to_string(built.join("songs/index.html")).expect("the published page is built");
    assert!(page.contains("Zebrafish listen"), "{page}");
    assert_eq!(std::fs::read(built.join("media/files/tune.mp3")).unwrap(), song, "the song on the site is the LFS file itself, not its pointer");
    assert!(built.join("pagefind/pagefind.js").is_file(), "the search index is built");
    let fragments: String = std::fs::read_dir(built.join("pagefind/fragment")).unwrap().flatten().map(|e| String::from_utf8_lossy(&crate::site_export::hugo_tests::unpack(&e.path())).into_owned()).collect();
    assert!(fragments.contains("\"url\":\"/songs/\"") && fragments.contains("Zebrafish listen"), "{fragments}");
    assert_eq!(std::fs::read_to_string(www.join("notes-releases/.built")).unwrap().trim(), tip(&git, &st.root, &url), "the commit that was built is remembered");

    // a second press with nothing new builds nothing
    dispatch_and_wait(&token, &builder, 2);
    assert_eq!(served(&www), built, "nothing new, nothing built");
    assert_eq!(releases(&www).len(), 1, "{:?}", releases(&www));

    // a new publish is built and swapped in; the old build stays for now
    f.file("Songs.md", "Quokka listen ![[tune.mp3]]\n");
    go(&git, &f, &base, false).unwrap();
    dispatch_and_wait(&token, &builder, 3);
    let newer = served(&www);
    assert_ne!(newer, built);
    assert!(std::fs::read_to_string(newer.join("songs/index.html")).unwrap().contains("Quokka listen") && built.is_dir());
    assert_eq!(std::fs::read(newer.join("media/files/tune.mp3")).unwrap(), song);
    assert_eq!(releases(&www).len(), 2, "{:?}", releases(&www));
}
