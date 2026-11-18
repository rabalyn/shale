//! Tidying the text of an imported e-mail (or event description): what HTML-to-text leaves behind
//! (IMP-053) while the signature delimiter stays exactly as it is (IMP-055).

/// The line that starts a signature (RFC 3676): two dashes and a space. The editor folds the text
/// from the last such line to the end of the message.
pub const SIGNATURE_DELIMITER: &str = "-- ";

/// A URL is shortened when it is longer than this and has a query string.
const LONG_URL: usize = 80;

/// The tidy text: line ends are `\n`; trailing spaces are gone (but not those of the `-- ` line,
/// and a bare `--` line becomes `-- `, with a blank line before it so that it is no Markdown
/// heading underline); runs of three or more blank lines are one; long tracking URLs are links
/// `[host](url)`; no blank lines at the start or end. Quoted lines (`> …`) stay as they are, apart
/// from the trailing spaces and long URLs.
pub fn tidy(text: &str) -> String {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut out: Vec<String> = Vec::new();
    let mut blank = 0usize;
    for line in text.split('\n') {
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            blank += 1;
            // the third blank line and the following ones are dropped
            if blank <= 2 {
                out.push(String::new());
            }
            continue;
        }
        if blank >= 3 {
            // the run is one blank line
            out.truncate(out.len() - 2);
            out.push(String::new());
        }
        blank = 0;
        if trimmed == "--" || trimmed == "-- " {
            if out.last().is_some_and(|l| !l.is_empty()) {
                out.push(String::new());
            }
            out.push(SIGNATURE_DELIMITER.to_string());
        } else {
            out.push(shorten_urls(trimmed));
        }
    }
    if blank >= 3 {
        out.truncate(out.len() - 2);
    }
    while out.last().is_some_and(String::is_empty) {
        out.pop();
    }
    while out.first().is_some_and(String::is_empty) {
        out.remove(0);
    }
    out.join("\n")
}

/// `url` without the punctuation of the sentence it ends: `.,;:!?` and a `)` that closes nothing.
fn trim_sentence(url: &str) -> &str {
    let mut url = url;
    loop {
        let open = url.matches('(').count();
        let close = url.matches(')').count();
        match url.chars().last() {
            Some('.' | ',' | ';' | ':' | '!' | '?') => url = &url[..url.len() - 1],
            Some(')') if close > open => url = &url[..url.len() - 1],
            _ => return url,
        }
    }
}

/// The long URLs with a query string in `line` that are not part of a Markdown link yet, as
/// `[host](url)`.
fn shorten_urls(line: &str) -> String {
    let mut out = String::new();
    let mut rest = line;
    while let Some(at) = rest.find("http") {
        let (before, here) = rest.split_at(at);
        let is_url = here.starts_with("http://") || here.starts_with("https://");
        let in_link = before.ends_with("](");
        let word_start = before.chars().last().is_none_or(|c| !c.is_alphanumeric());
        if !is_url || in_link || !word_start {
            out.push_str(before);
            out.push_str("http");
            rest = &here[4..];
            continue;
        }
        let end = here.find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | ']')).unwrap_or(here.len());
        let url = trim_sentence(&here[..end]);
        let after = &here[url.len()..];
        out.push_str(before);
        if url.len() > LONG_URL && url.contains('?') {
            // `<url>` of a plain-text mail: the brackets go
            if out.ends_with('<') && after.starts_with('>') {
                out.pop();
                rest = &after[1..];
            } else {
                rest = after;
            }
            out.push_str(&format!("[{}]({})", host_of(url), url.replace('(', "%28").replace(')', "%29")));
        } else {
            out.push_str(url);
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// "https://www.example.org/a?b" → "example.org".
fn host_of(url: &str) -> String {
    let after = url.split_once("://").map_or(url, |(_, r)| r);
    let host = after.split(['/', '?', '#']).next().unwrap_or(after);
    host.rsplit('@').next().unwrap_or(host).trim_start_matches("www.").to_string()
}
