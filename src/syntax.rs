//! Syntax highlighting of code (EDIT-032): fenced code blocks by the language on the opening
//! fence, inline code spans by a small language-neutral rule. The grammars are the ones of the
//! two-face crate (bat's set, parsed by syntect; no theme: the scopes it finds are sorted into a
//! few `Kind`s that `theme` colours).
//! The result for a block is cached, so a long note is not highlighted again on every frame.

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::sync::{Arc, Mutex, OnceLock};
use syntect::parsing::{ParseState, Scope, ScopeStack, SyntaxReference, SyntaxSet};

/// What a piece of code is; the colour of each comes from `Palette::syntax`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Kind {
    Keyword,
    String,
    Number,
    Comment,
    Function,
    Type,
    Constant,
    Operator,
}

/// Byte ranges (relative to the code) that are not plain text, in order, never overlapping.
pub type Spans = Vec<(Range<usize>, Kind)>;

/// Code longer than this many lines is shown plain (a pasted log must not stall the editor).
const MAX_LINES: usize = 5000;
/// Highlighted blocks kept; the cache starts over when it is full.
const CACHE_MAX: usize = 256;

/// One fenced block of a note.
#[derive(Debug, PartialEq, Eq)]
pub struct Block {
    /// the language word of the opening fence, lower case (empty: none given)
    pub lang: String,
    /// the lines between the fences (an unclosed block runs to the end of the text)
    pub body: Range<usize>,
}

/// The language word of an opening fence line: ```` ```js ````, ```` ``` Rust ````,
/// ```` ```{.python .numberLines} ````, ```` ```js {1,3} ````. Empty when there is none.
pub fn fence_language(line: &str) -> String {
    let info = line.trim().trim_start_matches(['`', '~']).trim_start();
    let info = info.trim_start_matches('{').trim_start().trim_start_matches('.');
    let word: String = info.chars().take_while(|c| !c.is_whitespace() && !matches!(c, '{' | '}' | ',')).collect();
    word.to_lowercase()
}

/// The fenced blocks of a note, by the same rule as `markdown::fenced_code_lines`.
pub fn fenced_blocks(text: &str) -> Vec<Block> {
    let mut out = Vec::new();
    let mut open: Option<(String, usize)> = None;
    let mut start = 0;
    for line in text.split('\n') {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            match open.take() {
                Some((lang, body)) => out.push(Block { lang, body: body..start }),
                None => open = Some((fence_language(line), start + line.len() + 1)),
            }
        }
        start += line.len() + 1;
    }
    if let Some((lang, body)) = open {
        out.push(Block { lang, body: body.min(text.len())..text.len() });
    }
    out
}

/// A fenced code block by lines (EDIT-080): from its opening fence line to the line after the closing one.
#[derive(Clone, Debug, PartialEq)]
pub struct Fence {
    /// the line of the opening fence (from 0)
    pub open: usize,
    /// the line after the closing fence; an unclosed block runs to the end (`lines.len()`)
    pub end: usize,
    pub closed: bool,
    /// the language word of the opening fence ([`fence_language`])
    pub lang: String,
}

/// The fenced blocks of these lines, by the same rule as `markdown::fenced_code_lines`.
pub fn fences(lines: &[&str]) -> Vec<Fence> {
    let mut out = Vec::new();
    let mut open: Option<usize> = None;
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            match open.take() {
                Some(o) => out.push(Fence { open: o, end: i + 1, closed: true, lang: fence_language(lines[o]) }),
                None => open = Some(i),
            }
        }
    }
    if let Some(o) = open {
        out.push(Fence { open: o, end: lines.len(), closed: false, lang: fence_language(lines[o]) });
    }
    out
}

/// What a code block's badge says (EDIT-080): the language as written (lower case); `None` when there
/// is none or it only means plain text (`text`, `txt`, `none`, the names the highlighter takes for no language).
pub fn badge_language(lang: &str) -> Option<String> {
    let word = lang.trim();
    (!word.is_empty() && !alias(word).is_empty()).then(|| word.to_string())
}

fn syntaxes() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(two_face::syntax::extra_newlines)
}

/// The names people give a language that the bundled grammars know under another one.
fn alias(word: &str) -> &str {
    match word {
        "jsx" | "mjs" | "cjs" | "node" | "javascript" => "js",
        "typescript" => "ts",
        "python" | "python3" | "py3" => "py",
        "rust" => "rs",
        "shell" | "zsh" | "console" | "sh-session" | "fish" => "sh",
        "c++" | "cxx" | "hpp" => "cpp",
        "csharp" | "c#" => "cs",
        "golang" => "go",
        "yml" => "yaml",
        "jsonc" | "json5" => "json",
        "md" => "markdown",
        "rb" => "ruby",
        "text" | "txt" | "plain" | "plaintext" | "none" | "nohighlight" => "",
        w => w,
    }
}

fn syntax_for(word: &str) -> Option<&'static SyntaxReference> {
    let word = alias(word);
    if word.is_empty() {
        return None;
    }
    let set = syntaxes();
    set.find_syntax_by_token(word).filter(|s| s.name != "Plain Text")
}

fn scope_kind(scope: &str) -> Option<Kind> {
    let is = |p: &str| scope == p || scope.starts_with(&format!("{p}."));
    Some(if is("comment") {
        Kind::Comment
    } else if is("string") {
        Kind::String
    } else if is("constant.numeric") {
        Kind::Number
    } else if is("constant") {
        Kind::Constant
    } else if is("keyword.operator") {
        Kind::Operator
    } else if is("keyword") || is("storage") {
        Kind::Keyword
    } else if is("entity.name.function") || is("support.function") || is("variable.function") {
        Kind::Function
    } else if is("entity.name") || is("support.type") || is("support.class") || is("entity.other.inherited-class") {
        Kind::Type
    } else {
        return None;
    })
}

/// The kind of the innermost scope that has one.
fn stack_kind(stack: &[Scope]) -> Option<Kind> {
    stack.iter().rev().find_map(|s| scope_kind(&s.to_string()))
}

fn run_syntect(syntax: &SyntaxReference, code: &str) -> Spans {
    let set = syntaxes();
    let mut state = ParseState::new(syntax);
    let mut stack = ScopeStack::new();
    let mut out: Spans = Vec::new();
    let mut base = 0;
    for line in code.split_inclusive('\n').take(MAX_LINES) {
        let Ok(ops) = state.parse_line(line, set) else { break };
        let mut kind = stack_kind(stack.as_slice());
        let mut pos = 0;
        for (at, op) in ops.iter().map(|(a, o)| (*a, o)).chain(std::iter::once((line.len(), &syntect::parsing::ScopeStackOp::Noop))) {
            push_span(&mut out, base + pos..base + at, kind);
            pos = at;
            if stack.apply(op).is_err() {
                return out;
            }
            kind = stack_kind(stack.as_slice());
        }
        base += line.len();
    }
    out
}

/// Add a span, joining it to the one before when they touch and are of one kind.
fn push_span(out: &mut Spans, r: Range<usize>, kind: Option<Kind>) {
    let Some(kind) = kind else { return };
    if r.is_empty() {
        return;
    }
    match out.last_mut() {
        Some((last, k)) if *k == kind && last.end == r.start => last.end = r.end,
        _ => out.push((r, kind)),
    }
}

/// The spans of `code` in the language `word`, and whether they came from the cache.
fn lookup(word: &str, code: &str) -> (Arc<Spans>, bool) {
    static CACHE: OnceLock<Mutex<HashMap<u64, Arc<Spans>>>> = OnceLock::new();
    let Some(syntax) = syntax_for(word) else { return (Arc::new(Vec::new()), true) };
    let mut h = DefaultHasher::new();
    (syntax.name.as_str(), code).hash(&mut h);
    let key = h.finish();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(hit) = cache.lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
        return (hit.clone(), true);
    }
    let spans = Arc::new(run_syntect(syntax, code));
    let mut map = cache.lock().unwrap_or_else(|e| e.into_inner());
    if map.len() >= CACHE_MAX {
        map.clear();
    }
    map.insert(key, spans.clone());
    (spans, false)
}

/// The coloured parts of a block of code in the language `word` (a fence's language word, any
/// alias). A language that is not known, or none, gives no spans: the block stays plain.
pub fn highlight(word: &str, code: &str) -> Arc<Spans> {
    lookup(word, code).0
}

/// The coloured parts of an inline code span, which has no language: quoted strings, numbers and
/// names that are called (`name(`). Anything else stays in the code colour.
pub fn highlight_inline(code: &str) -> Spans {
    let b = code.as_bytes();
    let mut out: Spans = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if matches!(c, b'"' | b'\'' | b'`') {
            // an unclosed quote is probably an apostrophe: plain
            let end = code[i + 1..].find(c as char).map(|e| i + 1 + e + 1);
            if let Some(end) = end {
                push_span(&mut out, i..end, Some(Kind::String));
                i = end;
                continue;
            }
            i += 1;
        } else if c.is_ascii_digit() {
            let end = i + b[i..].iter().take_while(|c| c.is_ascii_alphanumeric() || **c == b'.' || **c == b'_').count();
            push_span(&mut out, i..end, Some(Kind::Number));
            i = end;
        } else if c.is_ascii_alphabetic() || c == b'_' || c == b'$' {
            let end = i + b[i..].iter().take_while(|c| c.is_ascii_alphanumeric() || **c == b'_' || **c == b'$').count();
            if b.get(end) == Some(&b'(') {
                push_span(&mut out, i..end, Some(Kind::Function));
            }
            i = end;
        } else {
            i += code[i..].chars().next().map_or(1, char::len_utf8);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn language_name(word: &str) -> Option<&'static str> {
        syntax_for(word).map(|s| s.name.as_str())
    }

    fn kinds(spans: &Spans) -> Vec<Kind> {
        spans.iter().map(|(_, k)| *k).collect()
    }

    // spec: EDIT-032
    #[test]
    fn the_language_is_the_first_word_of_the_opening_fence() {
        assert_eq!(fence_language("```js"), "js");
        assert_eq!(fence_language("  ```Rust"), "rust");
        assert_eq!(fence_language("~~~ python title=\"x\""), "python");
        assert_eq!(fence_language("```{.python .numberLines}"), "python");
        assert_eq!(fence_language("```js {1,3}"), "js");
        assert_eq!(fence_language("```"), "");
        assert_eq!(fence_language("````"), "");
    }

    // spec: EDIT-032
    #[test]
    fn aliases_select_the_grammar_and_unknown_or_no_language_selects_none() {
        for (w, name) in [
            ("js", "JavaScript"),
            ("javascript", "JavaScript"),
            ("rs", "Rust"),
            ("rust", "Rust"),
            ("py", "Python"),
            ("python", "Python"),
            ("sh", "Bourne Again Shell (bash)"),
            ("bash", "Bourne Again Shell (bash)"),
            ("ts", "TypeScript"),
            ("typescript", "TypeScript"),
            ("tsx", "TypeScriptReact"),
            ("toml", "TOML"),
            ("c++", "C++"),
            ("yml", "YAML"),
            ("json", "JSON"),
        ] {
            assert_eq!(language_name(w), Some(name), "{w}");
        }
        for w in ["", "none", "text", "txt", "klingon", "mermaid"] {
            assert_eq!(language_name(w), None, "{w}");
        }
    }

    // spec: EDIT-032
    #[test]
    fn blocks_are_found_with_their_language_and_body() {
        let text = "a\n```js\nlet x = 1;\n```\nb\n~~~\nplain\n~~~\n```rust\nfn f() {}";
        let blocks = fenced_blocks(text);
        let bodies: Vec<(&str, &str)> = blocks.iter().map(|b| (b.lang.as_str(), &text[b.body.clone()])).collect();
        assert_eq!(bodies, [("js", "let x = 1;\n"), ("", "plain\n"), ("rust", "fn f() {}")], "the last block is not closed");
        assert!(fenced_blocks("```js").iter().all(|b| b.body.is_empty()), "an opening fence alone has an empty body");
    }

    // spec: EDIT-032
    #[test]
    fn known_languages_give_coloured_spans() {
        let code = "console.log(\"foooo\")\n";
        let spans = highlight("js", code);
        let text_of = |k: Kind| spans.iter().filter(|(_, x)| *x == k).map(|(r, _)| &code[r.clone()]).collect::<Vec<_>>();
        assert_eq!(text_of(Kind::String), ["\"foooo\""]);
        assert!(spans.len() >= 2, "more than one coloured span: {spans:?}");

        let rust = "// hi\nfn main() { let n = 42; }\n";
        let spans = highlight("rust", rust);
        let k = kinds(&spans);
        for want in [Kind::Comment, Kind::Keyword, Kind::Function, Kind::Number] {
            assert!(k.contains(&want), "{want:?} in {spans:?}");
        }
        assert!(spans.windows(2).all(|w| w[0].0.end <= w[1].0.start) && spans.iter().all(|(r, _)| rust.is_char_boundary(r.start) && rust.is_char_boundary(r.end)));
    }

    // spec: EDIT-032
    #[test]
    fn typescript_and_toml_have_grammars_of_their_own_and_get_sensible_kinds() {
        let ts = "interface A { n: number }\n// hi\nconst s: string = \"x\";\nfunction f(a: A): void {}\n";
        let spans = highlight("ts", ts);
        let text_of = |code: &str, spans: &Spans, k: Kind| spans.iter().filter(|(_, x)| *x == k).map(|(r, _)| code[r.clone()].to_string()).collect::<Vec<_>>();
        assert!(text_of(ts, &spans, Kind::Keyword).contains(&"interface".to_string()), "{spans:?}");
        assert!(text_of(ts, &spans, Kind::Type).iter().any(|t| t == "number" || t == "string"), "{spans:?}");
        assert_eq!(text_of(ts, &spans, Kind::String), ["\"x\""]);
        assert_eq!(text_of(ts, &spans, Kind::Comment), ["// hi"]);
        assert!(!highlight("tsx", "const e = <div id=\"a\">{1}</div>;\n").is_empty());

        let toml = "# conf\n[package]\nname = \"shale\"\nversion = 3\non = true\n";
        let spans = highlight("toml", toml);
        assert_eq!(text_of(toml, &spans, Kind::Comment), ["# conf"]);
        assert_eq!(text_of(toml, &spans, Kind::String), ["\"shale\""]);
        assert_eq!(text_of(toml, &spans, Kind::Number), ["3"]);
        assert!(text_of(toml, &spans, Kind::Type).contains(&"package".to_string()), "table name: {spans:?}");
        let kinds_found = kinds(&spans);
        assert!(kinds_found.contains(&Kind::Constant) || kinds_found.contains(&Kind::Keyword), "keys or booleans: {spans:?}");
    }

    // spec: EDIT-032
    #[test]
    fn unknown_languages_and_odd_code_stay_plain_and_never_panic() {
        assert!(highlight("klingon", "qapla'").is_empty());
        assert!(highlight("", "x = 1").is_empty());
        assert!(highlight("js", "").is_empty());
        let _ = highlight("rust", "fn é() { \"ünï\u{1F600}\" /* \n");
        let _ = highlight("js", "\r\nlet a = `x\r\n");
    }

    // spec: EDIT-032
    #[test]
    fn the_same_code_is_highlighted_once() {
        let code = "let cached_once_3141 = 'x';\n";
        let (a, hit_a) = lookup("js", code);
        let (b, hit_b) = lookup("javascript", code);
        assert!(!hit_a && hit_b, "the second look-up is a hit, whatever the alias");
        assert!(Arc::ptr_eq(&a, &b));
        assert!(!lookup("js", "let cached_once_2718 = 'x';\n").1, "other code is highlighted afresh");
        assert!(lookup("klingon", "x").1, "plain code costs nothing");
    }

    // spec: EDIT-032
    #[test]
    fn inline_code_colours_strings_numbers_and_called_names() {
        let code = "console.log(\"fooo\", 42)";
        let spans = highlight_inline(code);
        let got: Vec<(&str, Kind)> = spans.iter().map(|(r, k)| (&code[r.clone()], *k)).collect();
        assert_eq!(got, [("log", Kind::Function), ("\"fooo\"", Kind::String), ("42", Kind::Number)]);
        assert!(highlight_inline("plain words").is_empty());
        assert!(highlight_inline("it's é").is_empty(), "a lone quote is no string");
        assert!(highlight_inline("x1 = a2").is_empty(), "digits inside a name are not a number");
    }

    // spec: EDIT-080
    #[test]
    fn fences_are_found_by_line_with_their_language_and_an_unclosed_one_runs_to_the_end() {
        let lines = ["a", "```rust", "x", "```", "~~~", "y", "z"];
        assert_eq!(fences(&lines), vec![Fence { open: 1, end: 4, closed: true, lang: "rust".into() }, Fence { open: 4, end: 7, closed: false, lang: String::new() }]);
        assert!(fences(&["no", "code"]).is_empty());
    }

    // spec: EDIT-080
    #[test]
    fn the_badge_names_the_language_as_written_and_nothing_for_plain_text() {
        assert_eq!(badge_language("rust").as_deref(), Some("rust"));
        assert_eq!(badge_language("python").as_deref(), Some("python"));
        for none in ["", "text", "txt", "plaintext", "none"] {
            assert_eq!(badge_language(none), None, "{none:?}");
        }
    }
}
