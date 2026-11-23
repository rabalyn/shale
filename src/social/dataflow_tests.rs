//! The dataflow diagrams of the Mastodon and Bluesky flows (DEV-029), recorded from a run
//! against the pretend server of `fake_server.rs`: sign-in, the timelines to choose from and a
//! page of a timeline.

use super::fake_server::FakeServer;
use super::*;
use std::path::Path;

fn has_curl() -> bool {
    std::process::Command::new("curl").arg("--version").output().is_ok()
}

fn doc(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/dataflow").join(name)
}

// spec: DEV-026, DEV-029
#[test]
fn dev_029_dataflow_of_a_mastodon_sign_in_and_timeline() {
    if !has_curl() {
        return;
    }
    let server = FakeServer::start();
    let address = server.address();
    let flow = crate::dataflow::record(|| {
        let _flow = tracing::info_span!("flow.social_mastodon", peer = "app").entered();
        let app = mastodon::register_app(&address).unwrap();
        // the person's browser opens the sign-in page and shows the code
        let url = mastodon::authorize_url(&address, &app).unwrap();
        let (_, _, page) = tracing::info_span!("flow.social_mastodon.authorize", peer = "browser").in_scope(|| crate::mail::oauth_fake_tests::http_get(&url));
        let code = page.split_once("<code>").and_then(|(_, r)| r.split_once("</code>")).map(|(c, _)| c.to_string()).expect("the page shows a code");
        let (token, scope) = mastodon::exchange_code(&address, &app, &code).unwrap();
        assert_eq!(scope.as_deref(), Some("read write"));
        assert_eq!(mastodon::verify(&address, token.expose()).unwrap().handle, "ada@127.0.0.1");
        let feeds = mastodon::feeds(&address, token.expose()).unwrap();
        assert_eq!(feeds.len(), 2);
        let page = mastodon::timeline(&address, token.expose(), &feeds[0], None).unwrap();
        assert_eq!(page.posts.len(), 1);
    });
    crate::dataflow::verify(&doc("social-mastodon.md"), "social_mastodon", &crate::dataflow::render_mermaid(&flow));
}

// spec: DEV-026, DEV-029
#[test]
fn dev_029_dataflow_of_a_bluesky_sign_in_and_timeline() {
    if !has_curl() {
        return;
    }
    let server = FakeServer::start();
    let address = server.address();
    let flow = crate::dataflow::record(|| {
        let _flow = tracing::info_span!("flow.social_bluesky", peer = "app").entered();
        let session = bluesky::create_session(&address, "ada.test", "abcd-efgh-ijkl-mnop").unwrap();
        let mut api = bluesky::Api::new(&address, "ada.test", "abcd-efgh-ijkl-mnop");
        api.session = Some(session);
        let feeds = bluesky::feeds(&mut api).unwrap();
        assert_eq!(feeds.len(), 1);
        let page = bluesky::timeline(&mut api, &feeds[0], None).unwrap();
        assert_eq!(page.posts.len(), 1);
    });
    crate::dataflow::verify(&doc("social-bluesky.md"), "social_bluesky", &crate::dataflow::render_mermaid(&flow));
}
