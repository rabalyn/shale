//! Text conversions for posts: Mastodon's HTML to Markdown, and small helpers shared by both networks
//! (entities, safe link addresses, plain text of a Markdown post).

/// Decode HTML entities: the common named ones and `&#NN;` / `&#xNN;`. Unknown ones stay as written.
pub fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(p) = rest.find('&') {
        out.push_str(&rest[..p]);
        rest = &rest[p..];
        // an entity is short: look for the ';' within the next 10 characters
        let end = rest.char_indices().take(12).find(|(_, c)| *c == ';').map(|(i, _)| i);
        let decoded = end.and_then(|e| entity(&rest[1..e]).map(|d| (d, e)));
        match decoded {
            Some((d, e)) => {
                out.push_str(&d);
                rest = &rest[e + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn entity(name: &str) -> Option<String> {
    if let Some(num) = name.strip_prefix('#') {
        let n = match num.strip_prefix(['x', 'X']) {
            Some(h) => u32::from_str_radix(h, 16).ok()?,
            None => num.parse().ok()?,
        };
        return char::from_u32(n).filter(|c| *c != '\0').map(String::from);
    }
    let c = match name {
        "amp" => "&",
        "lt" => "<",
        "gt" => ">",
        "quot" => "\"",
        "apos" => "'",
        "nbsp" => " ",
        "hellip" => "…",
        "mdash" => "—",
        "ndash" => "–",
        "lsquo" => "‘",
        "rsquo" => "’",
        "ldquo" => "“",
        "rdquo" => "”",
        "laquo" => "«",
        "raquo" => "»",
        "copy" => "©",
        "reg" => "®",
        "trade" => "™",
        "euro" => "€",
        "bull" => "•",
        "middot" => "·",
        "deg" => "°",
        "times" => "×",
        "shy" => "",
        "zwj" => "\u{200d}",
        _ => return None,
    };
    Some(c.to_string())
}

/// A web address that is safe inside a Markdown link `[text](address)`: only http(s) and mailto
/// are kept; spaces and parentheses are written as %20 / %28 / %29. None for anything else.
pub fn md_url(href: &str) -> Option<String> {
    let h = href.trim();
    let lower = h.to_ascii_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://") || lower.starts_with("mailto:")) || h.chars().any(|c| c.is_control()) {
        return None;
    }
    Some(h.replace(' ', "%20").replace('(', "%28").replace(')', "%29").replace('<', "%3C").replace('>', "%3E"))
}

/// `[text](url)`, with brackets in the text escaped. Without a usable address just the text.
pub fn md_link(text: &str, href: &str) -> String {
    match md_url(href) {
        Some(u) => format!("[{}]({u})", text.replace('[', "\\[").replace(']', "\\]")),
        None => text.to_string(),
    }
}

/// One start or end tag.
struct Tag {
    name: String,
    closing: bool,
    attrs: Vec<(String, String)>,
}

impl Tag {
    fn attr(&self, k: &str) -> Option<&str> {
        self.attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }
}

/// Read a tag from the text after `<` (up to the `>`). Returns it and how many bytes it used.
fn read_tag(s: &str) -> Option<(Tag, usize)> {
    let b = s.as_bytes();
    // find the closing '>' that is not inside a quoted attribute value
    let (mut i, mut quote) = (0, 0u8);
    while i < b.len() {
        match (quote, b[i]) {
            (0, b'>') => break,
            (0, q @ (b'"' | b'\'')) => quote = q,
            (q, c) if q != 0 && c == q => quote = 0,
            _ => {}
        }
        i += 1;
    }
    if i >= b.len() {
        return None;
    }
    let inner = &s[..i];
    let closing = inner.starts_with('/');
    let inner = inner.trim_start_matches('/').trim_end_matches('/');
    let name_end = inner.find(|c: char| c.is_whitespace()).unwrap_or(inner.len());
    let name = inner[..name_end].to_ascii_lowercase();
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    let mut attrs = Vec::new();
    let mut rest = inner[name_end..].trim_start();
    while !rest.is_empty() {
        let key_end = rest.find(|c: char| c == '=' || c.is_whitespace()).unwrap_or(rest.len());
        let key = rest[..key_end].to_ascii_lowercase();
        rest = rest[key_end..].trim_start();
        let mut val = String::new();
        if let Some(r) = rest.strip_prefix('=') {
            let r = r.trim_start();
            if let Some(q) = r.chars().next().filter(|c| *c == '"' || *c == '\'') {
                let body = &r[1..];
                let e = body.find(q).unwrap_or(body.len());
                val = decode_entities(&body[..e]);
                rest = body.get(e + 1..).unwrap_or("").trim_start();
            } else {
                let e = r.find(char::is_whitespace).unwrap_or(r.len());
                val = decode_entities(&r[..e]);
                rest = r[e..].trim_start();
            }
        }
        if !key.is_empty() {
            attrs.push((key, val));
        }
    }
    Some((Tag { name, closing, attrs }, i + 1))
}

/// Mastodon's HTML (`content` of a status) as Markdown:
/// paragraphs, line breaks, list items, links (mentions and hashtags as `[@user](address)`,
/// shortened web addresses as the full address), entities decoded, other tags dropped.
pub fn html_to_markdown(html: &str) -> String {
    let mut out = String::new();
    // open links: (where their text starts in `out`, address, class, saw an invisible/ellipsis span)
    let mut links: Vec<(usize, String, String, bool)> = Vec::new();
    // open <span> elements inside a link: whether each hides or shortens text
    let mut spans: Vec<bool> = Vec::new();
    let mut rest = html;
    while let Some(p) = rest.find('<') {
        out.push_str(&decode_entities(&rest[..p]));
        rest = &rest[p..];
        if let Some(r) = rest.strip_prefix("<!--") {
            rest = r.find("-->").map(|e| &r[e + 3..]).unwrap_or("");
            continue;
        }
        let Some((tag, used)) = read_tag(&rest[1..]) else {
            out.push('<');
            rest = &rest[1..];
            continue;
        };
        rest = &rest[1 + used..];
        match (tag.name.as_str(), tag.closing) {
            ("br", _) => out.push('\n'),
            ("p", true) => out.push_str("\n\n"),
            ("p", false) if !out.is_empty() && !out.ends_with("\n\n") => out.push_str("\n\n"),
            ("li", false) => {
                if !out.is_empty() && !out.ends_with('\n') {
                    out.push('\n');
                }
                out.push_str("- ");
            }
            ("li", true) => out.push('\n'),
            ("ul" | "ol", true) => out.push('\n'),
            ("a", false) => links.push((out.len(), tag.attr("href").unwrap_or("").to_string(), tag.attr("class").unwrap_or("").to_string(), false)),
            ("a", true) => {
                if let Some((start, href, class, shortened)) = links.pop() {
                    let text = out.split_off(start);
                    out.push_str(&link_text(&text, &href, &class, shortened));
                }
            }
            ("span", false) => {
                let c = tag.attr("class").unwrap_or("");
                let hides = c.split_whitespace().any(|w| w == "invisible" || w == "ellipsis");
                if hides {
                    if let Some(l) = links.last_mut() {
                        l.3 = true;
                    }
                }
                spans.push(hides);
            }
            ("span", true) => {
                spans.pop();
            }
            _ => {}
        }
    }
    out.push_str(&decode_entities(rest));
    tidy(&out)
}

/// What a finished link becomes.
fn link_text(text: &str, href: &str, class: &str, shortened: bool) -> String {
    let text = text.trim();
    let classes: Vec<&str> = class.split_whitespace().collect();
    let people_or_tag = classes.contains(&"mention") || classes.contains(&"hashtag");
    if href.trim().is_empty() {
        return text.to_string();
    }
    if people_or_tag || (!shortened && text != href.trim() && !text.is_empty() && !looks_like_address(text)) {
        return md_link(if text.is_empty() { href } else { text }, href);
    }
    // a plain web address (maybe shortened to "example.com/some…" for display): the full one
    md_url(href).map(|u| if u.starts_with("mailto:") { text.to_string() } else { u }).unwrap_or_else(|| text.to_string())
}

fn looks_like_address(t: &str) -> bool {
    t.starts_with("http://") || t.starts_with("https://") || t.starts_with("www.")
}

/// Trim, at most one empty line between paragraphs, no spaces at line ends.
fn tidy(s: &str) -> String {
    let lines: Vec<&str> = s.lines().map(|l| l.trim_end()).collect();
    let mut out = String::new();
    let mut blank = 0;
    for l in lines {
        if l.is_empty() {
            blank += 1;
            continue;
        }
        if !out.is_empty() {
            out.push_str(if blank > 0 { "\n\n" } else { "\n" });
        }
        blank = 0;
        out.push_str(l);
    }
    out.trim().to_string()
}

/// The words of a Markdown post as plain text (links become their text, bare addresses go),
/// all on one line. For titles and search.
pub fn plain_text(md: &str) -> String {
    let mut out = String::new();
    let mut rest = md;
    while let Some(p) = rest.find('[') {
        out.push_str(&rest[..p]);
        let after = &rest[p + 1..];
        // [text](address)
        if let Some(close) = after.find("](") {
            if !after[..close].contains('\n') {
                if let Some(end) = after[close + 2..].find(')') {
                    out.push_str(&after[..close].replace("\\[", "[").replace("\\]", "]"));
                    rest = &after[close + 2 + end + 1..];
                    continue;
                }
            }
        }
        out.push('[');
        rest = after;
    }
    out.push_str(rest);
    out.split_whitespace().filter(|w| !looks_like_address(w)).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entities() {
        assert_eq!(decode_entities("a &amp; b &lt;c&gt; &quot;d&quot; &#39;e&#39; &#x1F600; &hellip; &unknown; & x &#xZZ;"), "a & b <c> \"d\" 'e' 😀 … &unknown; & x &#xZZ;");
        assert_eq!(decode_entities("&amp;amp;"), "&amp;");
        assert_eq!(decode_entities("Grüße &nbsp;ü"), "Grüße  ü");
    }

    #[test]
    fn paragraphs_and_line_breaks() {
        assert_eq!(html_to_markdown("<p>First line<br>second line</p><p>Second paragraph</p>"), "First line\nsecond line\n\nSecond paragraph");
        assert_eq!(html_to_markdown("<p>one</p>\n\n<p></p><p>two &amp; three</p>"), "one\n\ntwo & three");
        assert_eq!(html_to_markdown("plain text with <b>bold</b> and <script>x</script>"), "plain text with bold and x");
        assert_eq!(html_to_markdown("<p>a < b and 3 > 2</p>"), "a < b and 3 > 2");
        assert_eq!(html_to_markdown("<ul><li>one</li><li>two</li></ul>"), "- one\n- two");
        assert_eq!(html_to_markdown("<p>x<!-- hidden --> y</p>"), "x y");
    }

    #[test]
    fn mastodon_links() {
        let html = r#"<p><span class="h-card" translate="no"><a href="https://fosstodon.org/@ada" class="u-url mention">@<span>ada</span></a></span> look at <a href="https://mastodon.social/tags/rust" class="mention hashtag" rel="tag">#<span>rust</span></a> and <a href="https://example.com/a/very/long/path?x=1&amp;y=2" target="_blank" rel="nofollow noopener noreferrer" translate="no"><span class="invisible">https://</span><span class="ellipsis">example.com/a/very/lo</span><span class="invisible">ng/path?x=1&amp;y=2</span></a></p>"#;
        assert_eq!(html_to_markdown(html), "[@ada](https://fosstodon.org/@ada) look at [#rust](https://mastodon.social/tags/rust) and https://example.com/a/very/long/path?x=1&y=2");
        // a link with its own text (from other software) keeps the text
        assert_eq!(html_to_markdown(r#"<p>see <a href="https://example.com/x">the docs [v2]</a></p>"#), "see [the docs \\[v2\\]](https://example.com/x)");
        // the address as text
        assert_eq!(html_to_markdown(r#"<a href="https://example.com">https://example.com</a>"#), "https://example.com");
        // bad schemes are dropped, spaces and brackets in addresses made safe
        assert_eq!(html_to_markdown(r#"<a href="javascript:alert(1)">click</a>"#), "click");
        assert_eq!(html_to_markdown(r#"<a href="https://e.com/a b(1)">x</a>"#), "[x](https://e.com/a%20b%281%29)");
        // a link that is never closed doesn't hang or lose the text
        assert_eq!(html_to_markdown(r#"<a href="https://e.com/">open"#), "open");
    }

    #[test]
    fn markdown_helpers() {
        assert_eq!(md_link("a]b", "https://x.org/"), "[a\\]b](https://x.org/)");
        assert_eq!(md_link("t", "ftp://x"), "t");
        assert_eq!(plain_text("Hello [@ada](https://x.org/@ada), see https://example.com/a and [docs \\[v2\\]](https://e.com/) [not a link"), "Hello @ada, see and docs [v2] [not a link");
    }
}
