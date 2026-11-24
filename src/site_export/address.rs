//! The address of the site (SITE-080, SITE-081): where the built site is served, e.g.
//! `https://wiki.example.org/`. It is optional; with it the site has a `sitemap.xml` of absolute
//! addresses and a `robots.txt`, without it the pages link to each other by path only.

/// The address as the config keeps it, or `None` for no address. Only `http://` and `https://`,
/// a host, an optional port and path; no user name or password (SEC-010), no query or fragment, no
/// space. The path ends in a `/` (Hugo builds the links of a site below it).
pub fn check(text: &str) -> Result<Option<String>, String> {
    let url = text.trim();
    if url.is_empty() {
        return Ok(None);
    }
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("The address of the site can't contain spaces or control characters".into());
    }
    let lower = url.to_ascii_lowercase();
    let Some(rest) = lower.strip_prefix("https://").or_else(|| lower.strip_prefix("http://")) else {
        return Err("The address of the site must start with https:// or http://".into());
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.contains('@') {
        return Err("The address of the site can't have a user name or password in it".into());
    }
    let host = if authority.starts_with('[') { authority.split_inclusive(']').next().unwrap_or("") } else { authority.split(':').next().unwrap_or("") };
    if host.is_empty() {
        return Err("The address of the site has no host name".into());
    }
    if url.contains(['?', '#']) {
        return Err("The address of the site can't have a query or a fragment (? or #)".into());
    }
    Ok(Some(if url.ends_with('/') { url.to_string() } else { format!("{url}/") }))
}

#[cfg(test)]
mod tests {
    use super::check;

    // spec: SITE-081
    #[test]
    fn site_081_the_address_is_http_or_https_without_a_password_and_ends_in_a_slash() {
        assert_eq!(check("").unwrap(), None);
        assert_eq!(check("   ").unwrap(), None);
        assert_eq!(check(" https://wiki.example.org ").unwrap().as_deref(), Some("https://wiki.example.org/"));
        assert_eq!(check("https://host.example/wiki/").unwrap().as_deref(), Some("https://host.example/wiki/"));
        assert_eq!(check("http://localhost:8080/a/b").unwrap().as_deref(), Some("http://localhost:8080/a/b/"));
        assert_eq!(check("HTTPS://Wiki.Example.org").unwrap().as_deref(), Some("HTTPS://Wiki.Example.org/"));
        for bad in [
            "wiki.example.org",
            "ftp://example.org/",
            "file:///tmp/x",
            "ssh://git@example.org/x",
            "https://",
            "https:///x",
            "https://me@example.org/",
            "https://me:secret@example.org/",
            "https://example.org/a b",
            "https://example.org/?q=1",
            "https://example.org/#top",
            "https://exam\nple.org/",
        ] {
            let e = check(bad).expect_err(bad);
            assert!(!e.contains("secret"), "the message does not repeat the password: {e}");
        }
    }
}
