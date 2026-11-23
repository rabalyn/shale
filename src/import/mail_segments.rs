//! The text of an e-mail as segments (IMP-063) and the signature of each segment (IMP-064): the
//! sender's own text, then each quoted mail with its depth. What the Import dialog's "Skip
//! signatures" and "Leave out quoted mails" do to the text (IMP-065, IMP-066), and what the editor
//! folds inside quotes (EDIT-083). Whatever is not clearly a signature stays.

/// What the Import dialog cuts out of the text of an e-mail (IMP-065, IMP-066).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextCut {
    /// the own signature and the signatures inside the quoted mails go; the quoted text stays
    pub skip_signatures: bool,
    /// the quoted mails go altogether
    pub leave_out_quoted: bool,
}

/// One part of a message: the own text (`depth` 0) or a quoted mail.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Segment {
    /// 0 for the own text, 1 for a mail it quotes, 2 for the one that mail quotes, …
    pub depth: usize,
    /// how many lines at the start are the reply header ("On … wrote:", `-----Original Message-----`,
    /// the From / Sent / To / Subject block); 0 when there is none
    pub header_lines: usize,
    /// the lines as they are, with their line ends
    pub text: String,
}

/// Further than this many lines after a delimiter, it is no signature (RFC 3676 asks for four).
const LONGEST_SIGNATURE: usize = 40;
/// The most lines of a signature found by its closing greeting.
const LONGEST_GREETING_BLOCK: usize = 12;
/// A line of a signature block is short.
const LONGEST_SIGNATURE_LINE: usize = 70;
/// The most lines of a disclaimer at the end of a segment.
const LONGEST_DISCLAIMER: usize = 25;

/// Quote depth of a line and the line without its `>` marks: `> > a` → (2, `a`), `a` → (0, `a`).
pub fn quote_split(line: &str) -> (usize, &str) {
    let mut rest = line;
    let mut depth = 0;
    loop {
        let t = rest.trim_start();
        match t.strip_prefix('>') {
            Some(r) => {
                depth += 1;
                rest = r.strip_prefix(' ').unwrap_or(r);
            }
            None => return (depth, rest),
        }
    }
}

fn content(line: &str) -> &str {
    quote_split(line.trim_end_matches(['\n', '\r'])).1
}

fn lines_of(text: &str) -> Vec<&str> {
    text.split_inclusive('\n').collect()
}

/// A line that is the delimiter of RFC 3676 once the quote marks are gone: `-- ` or `--`.
pub fn is_delimiter(line: &str) -> bool {
    content(line).trim_end() == "--"
}

// ---------------------------------------------------------------- reply headers

/// "On … wrote:" and its German and French forms (`lines[0]` and, when it is wrapped, the next).
fn wrote_header(lines: &[&str]) -> Option<usize> {
    let c = content(lines.first()?).trim();
    let german = c.starts_with("Am ");
    let ends: &[&str] = if c.starts_with("On ") {
        &["wrote:"]
    } else if german {
        &["schrieb:", "geschrieben:"]
    } else if c.starts_with("Le ") {
        &["a écrit :", "a écrit:"]
    } else {
        return None;
    };
    let mut joined = String::new();
    for (n, l) in lines.iter().take(3).enumerate() {
        let c = content(l).trim();
        if c.is_empty() {
            return None;
        }
        joined.push(' ');
        joined.push_str(c);
        let j = joined.trim();
        let by_end = ends.iter().any(|e| j.ends_with(e)) || (german && j.ends_with(':') && j.contains(" schrieb"));
        if by_end && j.len() < 300 {
            return Some(n + 1);
        }
    }
    None
}

fn original_message(c: &str) -> bool {
    let dashes = c.chars().take_while(|ch| *ch == '-').count();
    if dashes < 3 || !c.trim_end().ends_with("--") {
        return false;
    }
    let name = c.trim_matches(|ch: char| ch == '-' || ch.is_whitespace()).to_lowercase();
    matches!(name.as_str(), "original message" | "ursprüngliche nachricht" | "originalnachricht" | "original-nachricht" | "originalmeldung" | "message d'origine")
}

fn header_key(c: &str) -> Option<&'static str> {
    let lower = c.trim().to_lowercase();
    let key = lower.split(':').next()?;
    if lower.len() == key.len() {
        return None;
    }
    ["from", "von", "sent", "gesendet", "date", "datum", "to", "an", "cc", "subject", "betreff", "reply-to", "importance", "wichtigkeit"].into_iter().find(|k| *k == key.trim())
}

/// The From / Sent / To / Subject block of Outlook (and Von / Gesendet / An / Betreff): lines that
/// start at `from` and are `Key: value` lines; the count of them, or 0.
fn header_block(lines: &[&str]) -> usize {
    let first = lines.first().map(|l| content(l)).and_then(header_key);
    if !matches!(first, Some("from" | "von")) {
        return 0;
    }
    let n = lines.iter().take_while(|l| header_key(content(l)).is_some()).count();
    if n >= 3 {
        n
    } else {
        0
    }
}

/// Where the reply header starts at `lines[0]`: the number of lines it takes and whether the mail
/// that follows is quoted by `>` marks anyway.
fn reply_header(lines: &[&str], before: &[&str]) -> Option<(usize, bool)> {
    let c = content(lines.first()?).trim();
    if let Some(n) = wrote_header(lines) {
        return Some((n, true));
    }
    if original_message(c) {
        let block = header_block(&lines[1..]);
        return Some((1 + block, false));
    }
    if c.len() >= 10 && c.chars().all(|ch| ch == '_') {
        let block = header_block(&lines[1..]);
        return (block > 0).then_some((1 + block, false));
    }
    let block = header_block(lines);
    if block > 0 {
        // a forwarded message is no reply: its header is part of the text
        let forwarded = before.iter().rev().filter(|l| !content(l).trim().is_empty()).take(2).any(|l| {
            let l = content(l).to_lowercase();
            l.contains("forwarded message") || l.contains("weitergeleitete nachricht") || l.contains("begin forwarded")
        });
        return (!forwarded).then_some((block, false));
    }
    None
}

// ---------------------------------------------------------------- segments

/// `text` as segments: the own text, then each quoted mail. They are made of whole lines and
/// follow each other, so [`join`] gives `text` back. A new segment starts at a reply header, and
/// where the quote depth of the text changes (`>` marks, or an unquoted reply header of Outlook
/// that quotes everything below it one level deeper); lines without text join the segment before.
pub fn split(text: &str) -> Vec<Segment> {
    let lines = lines_of(text);
    let mut segs: Vec<Segment> = Vec::new();
    let mut cur = Segment { depth: 0, header_lines: 0, text: String::new() };
    let mut has_text = false;
    let mut implied = 0usize;
    let mut i = 0;
    while i < lines.len() {
        if let Some((n, quoted)) = reply_header(&lines[i..], &lines[..i]) {
            let here = quote_split(lines[i]).0 + implied;
            let depth = match lines[i + n..].iter().find(|l| !content(l).trim().is_empty()) {
                Some(next) if quoted && quote_split(next).0 + implied > here => quote_split(next).0 + implied,
                _ => {
                    implied += 1;
                    here + 1
                }
            };
            if !cur.text.is_empty() {
                segs.push(cur);
            }
            cur = Segment { depth, header_lines: n, text: lines[i..i + n].concat() };
            has_text = true;
            i += n;
            continue;
        }
        let l = lines[i];
        if !content(l).trim().is_empty() {
            let depth = quote_split(l).0 + implied;
            if !has_text {
                cur.depth = depth;
                has_text = true;
            } else if depth != cur.depth {
                segs.push(cur);
                cur = Segment { depth, header_lines: 0, text: String::new() };
            }
        }
        cur.text.push_str(l);
        i += 1;
    }
    if !cur.text.is_empty() {
        segs.push(cur);
    }
    segs
}

/// The segments in one text again.
#[cfg(test)]
pub fn join(segments: &[Segment]) -> String {
    segments.iter().map(|s| s.text.as_str()).collect()
}

/// Answers between the quoted lines: an own segment without a header that has a quote before and
/// after it. There, a closing greeting cannot be told from one in the middle of the text.
fn is_inline(segs: &[Segment]) -> bool {
    segs.iter().enumerate().any(|(i, s)| s.depth == 0 && s.header_lines == 0 && segs[..i].iter().any(|p| p.depth > 0) && segs[i + 1..].iter().any(|n| n.depth > 0))
}

// ---------------------------------------------------------------- signatures

/// Closing greetings, lower case, without the punctuation at their end.
const GREETINGS: [&str; 22] = [
    "viele grüße",
    "viele gruesse",
    "beste grüße",
    "liebe grüße",
    "herzliche grüße",
    "freundliche grüße",
    "schöne grüße",
    "sonnige grüße",
    "mit freundlichen grüßen",
    "mit freundlichem gruß",
    "mit besten grüßen",
    "freundlichen gruß",
    "grüße",
    "gruß",
    "best regards",
    "kind regards",
    "warm regards",
    "regards",
    "best wishes",
    "yours sincerely",
    "sincerely",
    "cheers",
];

const DISCLAIMERS: [&str; 10] = [
    "this e-mail may contain confidential",
    "this email may contain confidential",
    "this e-mail and any attachments",
    "this email and any attachments",
    "this message is confidential",
    "diese e-mail enthält vertrauliche",
    "diese e-mail (einschließlich",
    "diese nachricht enthält vertrauliche",
    "confidentiality notice",
    "haftungsausschluss",
];

fn is_greeting(c: &str) -> bool {
    let c = c.trim().trim_end_matches([',', '!', '.', ';']).trim().to_lowercase();
    GREETINGS.contains(&c.as_str())
}

fn is_disclaimer_start(c: &str) -> bool {
    let c = c.trim().to_lowercase();
    DISCLAIMERS.iter().any(|d| c.starts_with(d))
}

/// A line that says how to reach the sender: an address, a phone number, a web site, a postal
/// code with its town, a company form.
fn is_contact(c: &str) -> bool {
    let lower = c.trim().to_lowercase();
    if lower.contains('@') || lower.contains("http://") || lower.contains("https://") || lower.contains("www.") {
        return true;
    }
    let label = ["tel", "fon", "phone", "mobil", "mobile", "fax", "telefon", "handy", "t:", "m:", "e:", "f:", "mob"];
    if label.iter().any(|w| lower.strip_prefix(w).is_some_and(|r| r.starts_with([':', ' ', '.', '/', '+']))) {
        return true;
    }
    let digits = lower.chars().filter(char::is_ascii_digit).count();
    let phone_like = lower.chars().all(|ch| ch.is_ascii_digit() || " +()-/.".contains(ch));
    if phone_like && digits >= 6 {
        return true;
    }
    let words: Vec<&str> = lower.split_whitespace().collect();
    if words.iter().any(|w| matches!(w.trim_end_matches([',', '.']), "gmbh" | "mbh" | "ag" | "inc" | "ltd" | "llc" | "kg" | "ug" | "e.v" | "ohg" | "gbr")) {
        return true;
    }
    // "12345 Berlin"
    words.windows(2).any(|w| w[0].len() == 5 && w[0].bytes().all(|b| b.is_ascii_digit()) && w[1].chars().next().is_some_and(char::is_alphabetic))
}

/// A line of one to three words of letters: a name.
fn is_name(c: &str) -> bool {
    let words: Vec<&str> = c.split_whitespace().collect();
    (1..=3).contains(&words.len()) && words.iter().all(|w| w.chars().all(|ch| ch.is_alphabetic() || matches!(ch, '.' | '-' | '\'')))
}

/// The first line of the signature of a segment, as a line number in `seg.text`; `None` when it
/// has none or when it is not sure. The delimiter line of RFC 3676 (outside a fenced code block,
/// with at most 40 lines after it) is sure; `heuristics` also looks for a closing greeting
/// followed by a short block with a way to contact the sender, and for a disclaimer at the end.
/// Both run to the end of the segment.
pub fn signature_start(seg: &Segment, heuristics: bool) -> Option<usize> {
    let lines = lines_of(&seg.text);
    let body = seg.header_lines.min(lines.len());
    let used = |from: usize| lines[from..].iter().filter(|l| !content(l).trim().is_empty()).count();
    let mut fenced = false;
    for (n, l) in lines.iter().enumerate().skip(body) {
        let c = content(l).trim_start();
        if c.starts_with("```") || c.starts_with("~~~") {
            fenced = !fenced;
        } else if !fenced && is_delimiter(l) && used(n + 1) <= LONGEST_SIGNATURE {
            return Some(n);
        }
    }
    if !heuristics {
        return None;
    }
    // a disclaimer at the end of the segment (after a blank line)
    let disclaimer = (body..lines.len()).find(|&n| is_disclaimer_start(content(lines[n])) && (n == body || content(lines[n - 1]).trim().is_empty() || is_contact(content(lines[n - 1]))) && lines.len() - n <= LONGEST_DISCLAIMER);
    let end = disclaimer.unwrap_or(lines.len());
    for g in (body..end).rev() {
        if !is_greeting(content(lines[g])) {
            continue;
        }
        let block: Vec<usize> = (g + 1..end).filter(|&n| !content(lines[n]).trim().is_empty()).collect();
        let ok = !block.is_empty() && block.len() <= LONGEST_GREETING_BLOCK && block.iter().all(|&n| content(lines[n]).trim().chars().count() <= LONGEST_SIGNATURE_LINE) && block.iter().any(|&n| is_contact(content(lines[n])));
        if ok {
            // the greeting and a name below it stay; the lines after them are the signature
            let name = block.first().filter(|&&n| is_name(content(lines[n])) && block.len() > 1);
            return Some(name.map_or(block[0], |_| block[1]));
        }
        break;
    }
    disclaimer
}

/// The text with the signature of every segment taken out (the lines from its start to the last
/// text of the segment; blank lines at its end stay), and the quoted mails too when `cut` says so.
/// With only the own text left empty, the quoted mails are kept.
pub fn cut(text: &str, cut: TextCut) -> String {
    if !cut.skip_signatures && !cut.leave_out_quoted {
        return text.to_string();
    }
    let segs = split(text);
    let inline = is_inline(&segs);
    let own_has_text = segs.iter().any(|s| s.depth == 0 && !s.text.trim().is_empty());
    let mut out = String::new();
    for seg in &segs {
        if cut.leave_out_quoted && seg.depth > 0 && own_has_text {
            continue;
        }
        let start = if cut.skip_signatures { signature_start(seg, !inline) } else { None };
        match start {
            None => out.push_str(&seg.text),
            Some(s) => {
                let lines = lines_of(&seg.text);
                out.extend(lines[..s].iter().copied());
                let tail = lines.iter().rposition(|l| !l.trim().is_empty()).map_or(lines.len(), |p| p + 1);
                out.extend(lines[tail..].iter().copied().filter(|l| l.trim().is_empty()));
            }
        }
    }
    out
}

/// The lines of a signature inside a quote, for the editor's folds (EDIT-083): for each
/// delimiter line of RFC 3676 behind quote marks (`> -- `), the line numbers `start..end` of the
/// signature, which ends where the quote's depth changes or a reply header begins. `skip` says for
/// a line number whether it is code (no delimiter there).
pub fn quoted_signatures(lines: &[&str], skip: impl Fn(usize) -> bool) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    let mut fenced = false;
    for l in 0..lines.len() {
        let (depth, c) = quote_split(lines[l]);
        if depth > 0 && (c.trim_start().starts_with("```") || c.trim_start().starts_with("~~~")) {
            fenced = !fenced;
        }
        if depth == 0 || fenced || skip(l) || !is_delimiter(lines[l]) || out.last().is_some_and(|r| l < r.1) {
            continue;
        }
        let end = (l + 1..lines.len())
            .find(|&n| {
                let (d, c) = quote_split(lines[n]);
                (!c.trim().is_empty() && d != depth) || reply_header(&lines[n..], &lines[..n]).is_some()
            })
            .unwrap_or(lines.len());
        out.push((l, end));
    }
    out
}
