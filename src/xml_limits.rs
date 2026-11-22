//! XML from outside the program (the parts of a Word file, a CalDAV server's answer) is parsed here, never with
//! `roxmltree::Document::parse` directly (DEV-038): roxmltree reads an element inside an element by calling itself, and has
//! no limit on how deep that goes, so a file of a few thousand `<a>` in a row overflows the stack and aborts the whole
//! program. [`parse`] first counts the depth of the tags in one pass that needs no stack, and refuses a document that is
//! nested more than [`MAX_DEPTH`] levels, with an error, before roxmltree sees it.

use roxmltree::Document;

/// The deepest nesting accepted. Word's own files stay below 50 (a table in a table in a content control…); roxmltree
/// overflows the 2 MiB stack of a thread at about 2 000 levels, so this leaves the rest of the stack to the code that
/// walks the tree afterwards.
pub const MAX_DEPTH: usize = 256;

/// Parse `text` as XML like `Document::parse`, but a document nested deeper than [`MAX_DEPTH`] is an error.
pub fn parse(text: &str) -> Result<Document<'_>, String> {
    check_depth(text)?;
    Document::parse(text).map_err(|e| e.to_string())
}

/// `Err` when the tags of `text` are nested deeper than [`MAX_DEPTH`]. It looks only at tags (skipping comments, CDATA,
/// processing instructions and declarations, quoted attribute values and `/>`), and what is not well-formed ends the
/// scan with `Ok`: roxmltree reports that.
pub fn check_depth(text: &str) -> Result<(), String> {
    let mut depth = 0usize;
    let mut at = 0;
    while let Some(found) = text[at..].find('<') {
        let start = at + found;
        let rest = &text[start..];
        let skip = |end: &str, from: usize| rest[from..].find(end).map(|i| start + from + i + end.len());
        let next = if rest.starts_with("<!--") {
            skip("-->", 4)
        } else if rest.starts_with("<![CDATA[") {
            skip("]]>", 9)
        } else if rest.starts_with("<?") {
            skip("?>", 2)
        } else if rest.starts_with("<!") {
            end_of_declaration(rest).map(|i| start + i)
        } else {
            let closing = rest.starts_with("</");
            let Some(end) = end_of_tag(rest) else { return Ok(()) };
            if closing {
                depth = depth.saturating_sub(1);
            } else if !rest[..end].ends_with("/>") {
                depth += 1;
                if depth > MAX_DEPTH {
                    return Err(format!("the XML is nested too deeply (more than {MAX_DEPTH} levels)"));
                }
            }
            Some(start + end)
        };
        match next {
            Some(n) => at = n,
            None => return Ok(()),
        }
    }
    Ok(())
}

/// The end (after the `>`) of the tag at the start of `rest`; a `>` inside a quoted attribute value does not count.
fn end_of_tag(rest: &str) -> Option<usize> {
    let bytes = rest.as_bytes();
    let mut i = 1;
    while i < bytes.len() {
        match bytes[i] {
            b'>' => return Some(i + 1),
            q @ (b'"' | b'\'') => i += rest[i + 1..].find(q as char)? + 1,
            _ => {}
        }
        i += 1;
    }
    None
}

/// The end of a `<!DOCTYPE …>`: a `>` inside `[…]` (the internal subset) or in quotes does not count.
fn end_of_declaration(rest: &str) -> Option<usize> {
    let bytes = rest.as_bytes();
    let (mut i, mut brackets) = (2, 0usize);
    while i < bytes.len() {
        match bytes[i] {
            b'>' if brackets == 0 => return Some(i + 1),
            b'[' => brackets += 1,
            b']' => brackets = brackets.saturating_sub(1),
            q @ (b'"' | b'\'') => i += rest[i + 1..].find(q as char)? + 1,
            _ => {}
        }
        i += 1;
    }
    None
}
