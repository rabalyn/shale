//! Syntax highlighting of code (EDIT-032): fenced code blocks by the language on the opening
//! fence, inline code spans by a small language-neutral rule. The grammars are the ones of the
//! two-face crate (bat's set, parsed by syntect; no theme: the scopes it finds are sorted into a
//! few `Kind`s that `theme` colours).
//! The result for a block is cached, so a long note is not highlighted again on every frame.

use crate::jobs;
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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

/// The cache of highlighted blocks: by grammar name and text.
fn cache() -> &'static Mutex<HashMap<u64, Arc<Spans>>> {
    static CACHE: OnceLock<Mutex<HashMap<u64, Arc<Spans>>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// The key of a block in the cache.
fn key_of(syntax: &SyntaxReference, code: &str) -> u64 {
    let mut h = DefaultHasher::new();
    (syntax.name.as_str(), code).hash(&mut h);
    h.finish()
}

/// Keep the spans of a block in the cache (it starts over when it is full).
fn store(key: u64, spans: &Arc<Spans>) {
    let mut map = cache().lock().unwrap_or_else(|e| e.into_inner());
    if map.len() >= CACHE_MAX {
        map.clear();
    }
    map.insert(key, spans.clone());
}

/// The spans of `code` in the language `word`, and whether they came from the cache.
fn lookup(word: &str, code: &str) -> (Arc<Spans>, bool) {
    let Some(syntax) = syntax_for(word) else { return (Arc::new(Vec::new()), true) };
    let key = key_of(syntax, code);
    if let Some(hit) = cache().lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
        return (hit.clone(), true);
    }
    let spans = Arc::new(run_syntect(syntax, code));
    store(key, &spans);
    (spans, false)
}

/// A block of this many lines or more is highlighted on a worker thread when it is drawn
/// ([`highlight_shown`], EDIT-121): about 0.23 ms a line on a warm grammar, so a block of 20 lines
/// is about 5 ms, which fits in a frame, and a longer one does not (dense lines cost several times that).
pub const ASYNC_LINES: usize = 20;
/// Blocks waiting for the worker; when more come (the text is being edited), the oldest are dropped.
const QUEUE_MAX: usize = 8;

/// The blocks waiting for the worker, and whether it is running.
#[derive(Default)]
struct Queue {
    waiting: VecDeque<(u64, &'static SyntaxReference, String)>,
    running: bool,
}

impl Queue {
    /// Add a block, unless it waits already; the oldest are dropped when there are more than [`QUEUE_MAX`].
    fn push(&mut self, key: u64, syntax: &'static SyntaxReference, code: &str) {
        if self.waiting.iter().any(|j| j.0 == key) {
            return;
        }
        self.waiting.push_back((key, syntax, code.to_string()));
        while self.waiting.len() > QUEUE_MAX {
            self.waiting.pop_front();
        }
    }
}

fn queue() -> &'static Mutex<Queue> {
    static QUEUE: OnceLock<Mutex<Queue>> = OnceLock::new();
    QUEUE.get_or_init(Default::default)
}

/// Counts the blocks the worker finished: whatever was drawn plain before is to be drawn again.
static GENERATION: AtomicU64 = AtomicU64::new(0);
/// Highlight everything on the calling thread (screenshots show the end state).
static ALL_SYNC: AtomicBool = AtomicBool::new(false);
/// Asks the window to repaint when the worker has a result.
static REPAINT: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

/// How many blocks the worker has finished so far. A layout made while this was another number
/// may show plain code that is coloured now (EDIT-121).
pub fn generation() -> u64 {
    GENERATION.load(Ordering::Acquire)
}

/// What to do when the worker has a result: repaint the window. Set once at start.
pub fn set_repaint(f: impl Fn() + Send + Sync + 'static) {
    let _ = REPAINT.set(Box::new(f));
}

/// Highlight every block on the calling thread, as the exports do: for screenshot runs, whose
/// picture must show the end state (DEV-007).
pub fn set_all_sync() {
    ALL_SYNC.store(true, Ordering::Release);
}

/// The worker: takes the newest waiting block (the one most likely still on the screen), highlights
/// it, keeps the result in the cache and asks for a repaint; ends when nothing waits.
fn work() {
    loop {
        let (key, syntax, code) = {
            let mut q = queue().lock().unwrap_or_else(|e| e.into_inner());
            match q.waiting.pop_back() {
                Some(job) => job,
                None => {
                    q.running = false;
                    return;
                }
            }
        };
        store(key, &Arc::new(run_syntect(syntax, &code)));
        GENERATION.fetch_add(1, Ordering::AcqRel);
        if let Some(f) = REPAINT.get() {
            f();
        }
    }
}

/// Put a block in the queue, unless it is in it already; start the worker when it is not running.
fn enqueue(key: u64, syntax: &'static SyntaxReference, code: &str) {
    let mut q = queue().lock().unwrap_or_else(|e| e.into_inner());
    q.push(key, syntax, code);
    if !q.running {
        q.running = true;
        // a panic of the grammar must not leave the worker marked as running
        jobs::run(work, |r| {
            if r.is_err() {
                let mut q = queue().lock().unwrap_or_else(|e| e.into_inner());
                q.running = false;
                q.waiting.clear();
            }
        });
    }
}

/// A block as it is drawn on the screen: its spans, and whether they are still to come.
pub struct Shown {
    pub spans: Arc<Spans>,
    /// the block is being highlighted on the worker; `spans` is empty (plain) until then
    pub pending: bool,
}

/// The coloured parts of a block for drawing (EDIT-121). A short block, a block in the cache and a
/// language that is not known are as [`highlight`]. A long block that is not in the cache is
/// plain at first and `pending`: it goes to the worker, which fills the cache, bumps
/// [`generation`] and repaints. The cache is by language and text, so a result for a text that was
/// edited meanwhile is never applied to the new text.
pub fn highlight_shown(word: &str, code: &str) -> Shown {
    shown(word, code, ALL_SYNC.load(Ordering::Acquire))
}

/// [`highlight_shown`]; `all_sync` highlights on the calling thread whatever the size.
fn shown(word: &str, code: &str, all_sync: bool) -> Shown {
    let done = |spans| Shown { spans, pending: false };
    let Some(syntax) = syntax_for(word) else { return done(Arc::new(Vec::new())) };
    if all_sync || code.lines().take(ASYNC_LINES).count() < ASYNC_LINES {
        return done(lookup(word, code).0);
    }
    let key = key_of(syntax, code);
    if let Some(hit) = cache().lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
        return done(hit.clone());
    }
    enqueue(key, syntax, code);
    Shown { spans: Arc::new(Vec::new()), pending: true }
}

/// The languages whose grammars are compiled in the background at start (EDIT-118), each with a
/// short sample that walks through the grammar's main contexts (strings, comments, numbers,
/// functions, types, templates): a grammar compiles its regexes when a context is first used.
const PREWARM: &[(&str, &str)] = &[
    ("ts", "import { a } from \"x\";\ninterface A<T> { n: number; s?: string }\n/* c */ // d\nexport class B<T> extends C implements A<T> {\n  private x: Map<string, number> = new Map();\n  async f(a: A, b = 1.5): Promise<void> { const t = `v ${a.n}`; if (a) { return await g(t, /re/g); } }\n}\nenum E { A = 1 }\ntype U = 'a' | 'b';\nconst fn = (x: number) => x * 2;\n"),
    ("tsx", "import React from \"react\";\nconst C = ({ a }: { a: number }) => <div id=\"a\" onClick={() => f(a)}>{a} text</div>;\n// c\nexport default C;\n"),
    ("js", "const a = 1; // c\n/* d */ function f(x, y = 2) { return `t ${x}` + \"s\" + 'q' + /re/g.test(y); }\nclass A extends B { static x = 3; get y() { return this.x; } }\nlet o = { k: [1, 2.5, 0xff], f() {} }; const g = async () => await f();\n"),
    ("rs", "use std::fmt;\n/// doc\n// c\n#[derive(Debug)]\npub struct A<'a> { n: &'a str, m: u32 }\nimpl<'a> A<'a> { pub fn f(&self, x: i64) -> Option<String> { let s = format!(\"{}\", x); match x { 0 => None, _ => Some(s) } } }\nfn main() { let v: Vec<u8> = vec![1, 2]; println!(\"{v:?}\"); let c = 'c'; let r = r#\"raw\"#; }\n"),
    ("py", "import os\n# c\n@dec\nclass A(B):\n    \"\"\"doc\"\"\"\n    def f(self, x: int = 1) -> str:\n        s = f\"v {x}\" + 'q'\n        return [i for i in range(3) if i]\nasync def g(): await h(1.5, None, True)\n"),
    ("sh", "#!/bin/sh\n# c\nset -eu\nf() { echo \"$1\" 'q' $(date) ${x:-1}; }\nif [ -f a ]; then f b; fi\nfor i in 1 2; do cat <<EOF\nhi\nEOF\ndone | grep x > /dev/null\n"),
    ("json", "{ \"a\": [1, 2.5, true, null], \"b\": { \"c\": \"d\" } }\n"),
    ("toml", "# c\n[package]\nname = \"x\"\nversion = 3\non = true\nd = 1979-05-27\n[[bin]]\nk = ['a', \"\"\"b\"\"\"]\n"),
    ("yaml", "# c\na: 1\nb:\n  - \"x\"\n  - 'y'\n  - true\nc: |\n  text\n"),
    ("cpp", "#include <vector>\n// c\n/* d */ template <typename T> class A : public B { public: int f(const T& x) const { return x + 1.5f; } };\nint main() { std::vector<int> v{1, 2}; auto s = \"s\"; char c = 'c'; return 0; }\n"),
    ("go", "package main\n// c\nimport \"fmt\"\ntype A struct { n int }\nfunc (a A) f(x int) string { return fmt.Sprintf(\"%d\", x) + `raw` }\nfunc main() { go f(); for i := 0; i < 2; i++ {} }\n"),
    ("html", "<!doctype html>\n<!-- c -->\n<div class=\"a\" id='b'><p>text &amp; more</p><script>let a = 1;</script><style>p { color: red; }</style></div>\n"),
    ("css", "/* c */\n.a > b:hover, #c { color: #fff; margin: 1px 2em; background: url(x.png); }\n@media (min-width: 1px) { p { top: 0 } }\n"),
    ("md", "# h\n\n*a* **b** `c` [l](u)\n\n- x\n\n```rs\nfn f() {}\n```\n"),
];

/// Compile the grammars of [`PREWARM`] and load the syntax set, here and now (EDIT-118). It does
/// not touch the cache of highlighted blocks. Returns how many grammars were warmed.
pub fn prewarm() -> usize {
    PREWARM.iter().filter_map(|(word, sample)| syntax_for(word).map(|s| run_syntect(s, sample))).count()
}

/// [`prewarm`] on a thread of its own, started once at start-up (EDIT-118): the first block of a
/// language then does not wait for its grammar to compile. A block that comes before the
/// thread is done is highlighted as before, so no frame ever shows wrong colours.
pub fn prewarm_in_background() {
    jobs::run(prewarm, |_| {});
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

    // spec: EDIT-118
    #[test]
    fn prewarm_warms_every_listed_grammar_without_touching_the_cache() {
        assert_eq!(prewarm(), PREWARM.len(), "every listed word has a grammar");
        for (word, sample) in PREWARM {
            assert!(!run_syntect(syntax_for(word).unwrap(), sample).is_empty(), "{word}: the sample has coloured parts");
        }
        let code = "let prewarm_not_cached_1618 = 'x';\n";
        prewarm();
        assert!(!lookup("js", code).1, "warming leaves the cache of blocks alone");
    }

    // spec: EDIT-118
    #[test]
    fn highlighting_while_the_background_warm_up_runs_gives_the_same_spans() {
        let code = "const racing_1414: number = 1; // c\n";
        let before = run_syntect(syntax_for("ts").unwrap(), code);
        let (tx, rx) = std::sync::mpsc::channel();
        jobs::run(prewarm, move |r| tx.send(r).unwrap());
        let during = run_syntect(syntax_for("ts").unwrap(), code);
        assert_eq!(during, before, "the result does not depend on who compiled the grammar");
        assert_eq!(rx.recv_timeout(std::time::Duration::from_secs(60)).unwrap(), Ok(PREWARM.len()));
        prewarm_in_background();
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

    /// A TypeScript block of `n` lines that no other test uses (`tag` makes the text its own).
    fn long_ts(tag: &str, n: usize) -> String {
        (0..n).map(|i| format!("const {tag}{i}: number = {i} + f(\"s{i}\"); // c\n")).collect()
    }

    /// Ask until the block is no longer pending (the worker has it), at most 30 s.
    fn settle(word: &str, code: &str) -> Shown {
        let end = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            let s = highlight_shown(word, code);
            if !s.pending || std::time::Instant::now() > end {
                return s;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    // spec: EDIT-121
    #[test]
    fn edit_121_a_short_block_is_highlighted_at_once_and_a_long_one_is_plain_then_coloured_by_the_worker() {
        let short = long_ts("short119", ASYNC_LINES - 1);
        let s = highlight_shown("ts", &short);
        assert!(!s.pending && !s.spans.is_empty(), "a block under the threshold is coloured on the first look");
        assert_eq!(s.spans, highlight("ts", &short));

        let long = long_ts("long119", ASYNC_LINES + 10);
        let before = generation();
        let first = highlight_shown("ts", &long);
        assert!(first.pending && first.spans.is_empty(), "plain at first");
        let done = settle("ts", &long);
        assert!(!done.pending && !done.spans.is_empty(), "coloured after the worker");
        assert!(generation() > before, "the generation says a block is done");
        assert_eq!(done.spans, run_syntect_for("ts", &long), "the same colours as the synchronous highlight");
        // an unknown language is never pending
        let s = highlight_shown("klingon", &long);
        assert!(!s.pending && s.spans.is_empty());
    }

    /// What the grammar of `word` makes of `code`, without the cache.
    fn run_syntect_for(word: &str, code: &str) -> Arc<Spans> {
        Arc::new(run_syntect(syntax_for(word).unwrap(), code))
    }

    // spec: EDIT-121
    #[test]
    fn edit_121_an_edited_block_never_gets_the_result_of_the_old_text() {
        let old = long_ts("old119", ASYNC_LINES + 5);
        let edited = old.replacen("old1190", "edited119x", 1);
        assert!(highlight_shown("ts", &old).pending);
        // the edit comes while the worker may still be busy with the old text
        let now = highlight_shown("ts", &edited);
        assert!(now.pending || now.spans != run_syntect_for("ts", &old), "never the spans of the old text");
        let done = settle("ts", &edited);
        assert_eq!(done.spans, run_syntect_for("ts", &edited));
        assert_ne!(done.spans, run_syntect_for("ts", &old), "the edit moved the spans");
    }

    // spec: EDIT-121
    #[test]
    fn edit_121_the_queue_is_bounded_keeps_the_newest_and_asks_once_per_block() {
        let ts = syntax_for("ts").unwrap();
        let mut q = Queue::default();
        for n in 0..QUEUE_MAX as u64 + 3 {
            q.push(n, ts, "");
        }
        assert_eq!(q.waiting.len(), QUEUE_MAX);
        assert_eq!(q.waiting.front().unwrap().0, 3, "the oldest were dropped");
        assert_eq!(q.waiting.back().unwrap().0, QUEUE_MAX as u64 + 2, "the newest is kept");
        q.push(5, ts, "");
        assert_eq!(q.waiting.len(), QUEUE_MAX, "a block that waits is not queued twice");
        // and the real queue answers for a block asked for again and again
        let code = long_ts("once119", ASYNC_LINES + 1);
        for _ in 0..5 {
            assert!(highlight_shown("ts", &code).pending || !highlight_shown("ts", &code).spans.is_empty());
        }
        settle("ts", &code);
    }

    // spec: EDIT-121
    #[test]
    fn edit_121_the_window_is_asked_to_repaint_when_the_worker_has_a_result_and_everything_can_be_sync() {
        static REPAINTS: AtomicU64 = AtomicU64::new(0);
        set_repaint(|| {
            REPAINTS.fetch_add(1, Ordering::SeqCst);
        });
        let code = long_ts("repaint119", ASYNC_LINES + 2);
        settle("ts", &code);
        assert!(REPAINTS.load(Ordering::SeqCst) >= 1, "a repaint was asked for");
        // the exports and screenshots highlight on the calling thread
        let other = long_ts("sync119", ASYNC_LINES + 2);
        let s = shown("ts", &other, true);
        assert!(!s.pending && !s.spans.is_empty());
        assert!(!highlight("ts", &long_ts("export119", ASYNC_LINES + 2)).is_empty(), "an export call is full at once");
    }
}
