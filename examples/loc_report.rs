//! A lines-of-code report in Markdown (DEV-044, #628).
//!
//! `cargo run --example loc_report -- [--root DIR] [--config FILE] [--out FILE]`
//!
//! Shale is a bin-only crate, so this program cannot use its code: it needs only `std`, `serde`
//! and `toml`. It follows the module tree from `src/main.rs`, so every `.rs` file of `src/` belongs
//! to one *feature* (its top-level module), counts each line as blank, comment, production code or
//! test code, groups the features into *featuresets* (`tools/loc-report/featuresets.toml`) and
//! writes the numbers as Markdown tables. The output has no timestamp, so the same tree gives the
//! same report.

use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// The pseudo-feature of the test modules that belong to no feature (`ui_test_support`).
const SHARED: &str = "Shared test support";

const USAGE: &str = "usage: cargo run --example loc_report -- [--root DIR] [--config FILE] [--out FILE]";

// ---- the lexer: which lines hold code, which only comments ----

/// A token of the little lexer: only what the module and `#[cfg(test)]` scans need.
#[derive(Debug, Clone, PartialEq)]
enum Tk {
    Ident(String),
    Punct(char),
    /// A plain string literal, its text between the quotes as written.
    Str(String),
    /// Any other literal: a number, a raw string, a char, a lifetime.
    Lit,
}

#[derive(Debug, Clone)]
struct Tok {
    kind: Tk,
    /// The 0-based line the token starts on.
    line: usize,
}

/// What a line holds besides whitespace.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct LineFlags {
    code: bool,
    comment: bool,
}

struct Lexed {
    tokens: Vec<Tok>,
    lines: Vec<LineFlags>,
}

struct Lexer {
    c: Vec<char>,
    i: usize,
    line: usize,
    out: Lexed,
}

impl Lexer {
    fn peek(&self, n: usize) -> Option<char> {
        self.c.get(self.i + n).copied()
    }

    /// Takes one character that belongs to code (a string's inside included), marking its line.
    fn bump_code(&mut self) -> char {
        self.out.lines[self.line].code = true;
        self.bump()
    }

    fn bump(&mut self) -> char {
        let ch = self.c[self.i];
        self.i += 1;
        if ch == '\n' {
            self.line += 1;
        }
        ch
    }

    fn push(&mut self, kind: Tk, line: usize) {
        self.out.tokens.push(Tok { kind, line });
    }

    fn line_comment(&mut self) {
        self.out.lines[self.line].comment = true;
        while self.peek(0).is_some_and(|ch| ch != '\n') {
            self.i += 1;
        }
    }

    /// A block comment, which nests in Rust: every line it spans holds comment text.
    fn block_comment(&mut self) {
        let mut depth = 0;
        while self.i < self.c.len() {
            self.out.lines[self.line].comment = true;
            match (self.c[self.i], self.peek(1)) {
                ('/', Some('*')) => {
                    depth += 1;
                    self.i += 2;
                }
                ('*', Some('/')) => {
                    depth -= 1;
                    self.i += 2;
                    if depth == 0 {
                        return;
                    }
                }
                _ => {
                    self.bump();
                }
            }
        }
    }

    /// A string with escapes, `self.i` at its opening quote.
    fn string(&mut self) {
        let line = self.line;
        self.bump_code();
        let mut text = String::new();
        while self.i < self.c.len() {
            let ch = self.bump_code();
            match ch {
                '"' => break,
                '\\' if self.i < self.c.len() => {
                    text.push(ch);
                    text.push(self.bump_code());
                }
                _ => text.push(ch),
            }
        }
        self.push(Tk::Str(text), line);
    }

    /// A raw string with `hashes` `#`, `self.i` at the first `#` or the opening quote.
    fn raw_string(&mut self, hashes: usize) {
        let line = self.line;
        for _ in 0..=hashes {
            self.bump_code();
        }
        while self.i < self.c.len() {
            if self.bump_code() == '"' && (0..hashes).all(|n| self.peek(n) == Some('#')) {
                for _ in 0..hashes {
                    self.bump_code();
                }
                break;
            }
        }
        self.push(Tk::Lit, line);
    }

    /// `'a'` and `'\n'` are chars, `'a` and `'static` are lifetimes; `self.i` is at the `'`.
    fn char_or_lifetime(&mut self) {
        let line = self.line;
        self.bump_code();
        if self.peek(0) == Some('\\') {
            // The backslash and the character it escapes (it may be a quote), then up to the end.
            self.bump_code();
            if self.i < self.c.len() {
                self.bump_code();
            }
            while self.i < self.c.len() && self.bump_code() != '\'' {}
        } else if self.peek(1) == Some('\'') && self.peek(0) != Some('\'') {
            self.bump_code();
            self.bump_code();
        } else {
            while self.peek(0).is_some_and(|ch| ch.is_alphanumeric() || ch == '_') {
                self.bump_code();
            }
        }
        self.push(Tk::Lit, line);
    }

    fn ident_chars(&mut self) -> String {
        let mut name = String::new();
        while self.peek(0).is_some_and(|ch| ch.is_alphanumeric() || ch == '_') {
            name.push(self.bump_code());
        }
        name
    }

    /// An identifier, or the prefix of a raw, byte or C string (`r#"`, `b"`, `br"`, `c"`) or of a
    /// byte char (`b'x'`).
    fn ident_or_prefixed(&mut self) {
        let line = self.line;
        let name = self.ident_chars();
        let hashes = (0..).take_while(|&n| self.peek(n) == Some('#')).count();
        match (name.as_str(), self.peek(0)) {
            ("r" | "br" | "cr", Some('"' | '#')) if self.peek(hashes) == Some('"') => self.raw_string(hashes),
            ("r", Some('#')) if hashes == 1 => {
                self.bump_code();
                let raw = self.ident_chars();
                self.push(Tk::Ident(raw), line);
            }
            ("b" | "c", Some('"')) => self.string(),
            ("b", Some('\'')) => self.char_or_lifetime(),
            _ => self.push(Tk::Ident(name), line),
        }
    }
}

/// Lexes Rust source into tokens and, for every line, whether it holds code and/or comment text.
fn lex(text: &str) -> Lexed {
    let lines = vec![LineFlags::default(); text.matches('\n').count() + 1];
    let mut lx = Lexer { c: text.chars().collect(), i: 0, line: 0, out: Lexed { tokens: Vec::new(), lines } };
    while let Some(ch) = lx.peek(0) {
        let line = lx.line;
        match ch {
            _ if ch.is_whitespace() => {
                lx.bump();
            }
            '/' if lx.peek(1) == Some('/') => lx.line_comment(),
            '/' if lx.peek(1) == Some('*') => lx.block_comment(),
            '"' => lx.string(),
            '\'' => lx.char_or_lifetime(),
            _ if ch.is_alphabetic() || ch == '_' => lx.ident_or_prefixed(),
            _ if ch.is_ascii_digit() => {
                while lx.peek(0).is_some_and(|d| d.is_alphanumeric() || d == '_') {
                    lx.bump_code();
                }
                lx.push(Tk::Lit, line);
            }
            _ => {
                lx.bump_code();
                lx.push(Tk::Punct(ch), line);
            }
        }
    }
    lx.out
}

// ---- attributes, `#[cfg(test)]` items and module declarations ----

/// An attribute `#[…]` or `#![…]` found in the tokens.
struct Attr {
    inner: bool,
    /// The tokens between the brackets.
    start: usize,
    end: usize,
    /// The index after the closing bracket.
    next: usize,
}

fn is_punct(t: &[Tok], at: usize, ch: char) -> bool {
    t.get(at).is_some_and(|tok| tok.kind == Tk::Punct(ch))
}

fn ident_at(t: &[Tok], at: usize) -> Option<&str> {
    match t.get(at).map(|tok| &tok.kind) {
        Some(Tk::Ident(name)) => Some(name),
        _ => None,
    }
}

fn attr_at(t: &[Tok], at: usize) -> Option<Attr> {
    if !is_punct(t, at, '#') {
        return None;
    }
    let inner = is_punct(t, at + 1, '!');
    let open = at + 1 + usize::from(inner);
    if !is_punct(t, open, '[') {
        return None;
    }
    let mut depth = 0;
    for (k, tok) in t.iter().enumerate().skip(open) {
        match tok.kind {
            Tk::Punct('[') => depth += 1,
            Tk::Punct(']') => {
                depth -= 1;
                if depth == 0 {
                    return Some(Attr { inner, start: open + 1, end: k, next: k + 1 });
                }
            }
            _ => {}
        }
    }
    None
}

/// Whether a `cfg` predicate is true only in a test build: `test`, `all(test, …)`; not `any(…)`
/// (it holds without `test`) and not `not(…)`.
fn pred_is_test(t: &[Tok], p: &mut usize) -> bool {
    let Some(name) = ident_at(t, *p) else {
        *p += 1;
        return false;
    };
    *p += 1;
    if is_punct(t, *p, '(') {
        *p += 1;
        let mut kids = Vec::new();
        while *p < t.len() && !is_punct(t, *p, ')') {
            kids.push(pred_is_test(t, p));
            if is_punct(t, *p, ',') {
                *p += 1;
            }
        }
        *p += 1;
        match name {
            "all" => kids.iter().any(|&k| k),
            "any" => !kids.is_empty() && kids.iter().all(|&k| k),
            _ => false,
        }
    } else if is_punct(t, *p, '=') {
        *p += 2;
        false
    } else {
        name == "test"
    }
}

/// Whether the tokens inside an attribute's brackets are a `cfg(…)` that holds only for tests.
fn cfg_is_test(attr: &[Tok]) -> bool {
    ident_at(attr, 0) == Some("cfg") && is_punct(attr, 1, '(') && pred_is_test(attr, &mut 2)
}

/// The end of the item that starts at `from` (after its attributes): the token that closes its
/// braces, ends it with `;` or `,`, or the one before the closer of the block it sits in.
fn item_end(t: &[Tok], from: usize) -> usize {
    let mut k = from;
    if ident_at(t, k) == Some("pub") {
        k += 1;
        if is_punct(t, k, '(') {
            while k < t.len() && !is_punct(t, k, ')') {
                k += 1;
            }
            k += 1;
        }
    }
    let kw = ident_at(t, k);
    let semi_only = match kw {
        Some("use" | "static" | "type" | "let") => true,
        Some("const") => !matches!(ident_at(t, k + 1), Some("fn" | "unsafe" | "async" | "extern")),
        _ => false,
    };
    // A field, a variant or an argument ends at its comma; an item keyword's commas are generics.
    let comma_ends = !semi_only && !matches!(kw, Some("fn" | "impl" | "struct" | "enum" | "trait" | "mod" | "extern" | "unsafe" | "async" | "union" | "macro_rules" | "const"));
    let mut depth = 0i32;
    let mut j = from;
    while j < t.len() {
        match t[j].kind {
            Tk::Punct('(' | '[' | '{') => depth += 1,
            Tk::Punct(')' | ']') | Tk::Punct('}') if depth == 0 => return j.saturating_sub(1),
            Tk::Punct(')' | ']') => depth -= 1,
            Tk::Punct('}') => {
                depth -= 1;
                if depth == 0 && !semi_only {
                    if ident_at(t, j + 1) == Some("else") {
                        j += 1;
                        continue;
                    }
                    return if is_punct(t, j + 1, ';') { j + 1 } else { j };
                }
            }
            Tk::Punct(';') if depth == 0 => return j,
            Tk::Punct(',') if depth == 0 && comma_ends => return j,
            _ => {}
        }
        j += 1;
    }
    t.len().saturating_sub(1)
}

/// The line ranges (0-based, inclusive) of the items marked `#[cfg(test)]`, attributes included.
/// An inner `#![cfg(test)]` marks the whole file.
fn test_ranges(t: &[Tok]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < t.len() {
        let Some(first) = attr_at(t, i) else {
            i += 1;
            continue;
        };
        if first.inner {
            if cfg_is_test(&t[first.start..first.end]) {
                out.push((0, usize::MAX));
            }
            i = first.next;
            continue;
        }
        let (mut j, mut test) = (i, false);
        while let Some(a) = attr_at(t, j).filter(|a| !a.inner) {
            test |= cfg_is_test(&t[a.start..a.end]);
            j = a.next;
        }
        if test {
            let end = item_end(t, j).max(i);
            out.push((t[i].line, t[end].line));
            i = end + 1;
        } else {
            i = j;
        }
    }
    out
}

/// A `mod name;` declaration of a file.
#[derive(Debug, Clone, PartialEq)]
struct ModDecl {
    name: String,
    /// Declared under a `#[cfg(test)]` (or `all(test, …)`) attribute.
    cfg_test: bool,
    /// The `#[path = "…"]` attribute, when there is one.
    path: Option<String>,
}

/// The `mod name;` declarations at the top level of a file (an inline `mod x { … }` has no file).
fn mod_decls(t: &[Tok]) -> Vec<ModDecl> {
    let mut out = Vec::new();
    let (mut cfg_test, mut path) = (false, None);
    let (mut depth, mut i) = (0i32, 0);
    while i < t.len() {
        if let Some(a) = attr_at(t, i) {
            let inner = &t[a.start..a.end];
            if depth == 0 && !a.inner {
                cfg_test |= cfg_is_test(inner);
                if let [Tok { kind: Tk::Ident(p), .. }, Tok { kind: Tk::Punct('='), .. }, Tok { kind: Tk::Str(s), .. }] = inner {
                    if p == "path" {
                        path = Some(s.clone());
                    }
                }
            }
            i = a.next;
            continue;
        }
        match &t[i].kind {
            Tk::Punct('{') | Tk::Punct('}') => {
                depth += if t[i].kind == Tk::Punct('{') { 1 } else { -1 };
                (cfg_test, path) = (false, None);
            }
            Tk::Punct(';') if depth == 0 => (cfg_test, path) = (false, None),
            Tk::Ident(kw) if kw == "mod" && depth == 0 && is_punct(t, i + 2, ';') => {
                if let Some(name) = ident_at(t, i + 1) {
                    out.push(ModDecl { name: name.to_string(), cfg_test, path: path.take() });
                }
                cfg_test = false;
                i += 2;
            }
            _ => {}
        }
        i += 1;
    }
    out
}

// ---- counting ----

/// Lines of one file by kind.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Counts {
    blank: usize,
    comment: usize,
    prod: usize,
    test: usize,
}

/// Counts the lines of a Rust file and lists its `mod x;` declarations. `whole_test`: the file's
/// module was declared under `#[cfg(test)]`, so all its code is test code.
fn analyze(text: &str, whole_test: bool) -> (Counts, Vec<ModDecl>) {
    let lexed = lex(text);
    let lines: Vec<&str> = text.lines().collect();
    let mut in_test = vec![whole_test; lines.len()];
    for (a, b) in test_ranges(&lexed.tokens) {
        for flag in in_test.iter_mut().take(b.saturating_add(1)).skip(a) {
            *flag = true;
        }
    }
    let mut counts = Counts::default();
    for (n, line) in lines.iter().enumerate() {
        if lexed.lines[n].code {
            if in_test[n] {
                counts.test += 1;
            } else {
                counts.prod += 1;
            }
        } else if line.trim().is_empty() {
            counts.blank += 1;
        } else {
            counts.comment += 1;
        }
    }
    (counts, mod_decls(&lexed.tokens))
}

/// Counts the lines of a shell script: `#` lines are comments (the shebang is code).
fn analyze_shell(text: &str) -> Counts {
    let mut counts = Counts::default();
    for (n, line) in text.lines().enumerate() {
        let t = line.trim();
        if t.is_empty() {
            counts.blank += 1;
        } else if t.starts_with('#') && !(n == 0 && t.starts_with("#!")) {
            counts.comment += 1;
        } else {
            counts.prod += 1;
        }
    }
    counts
}

/// The numbers of a row of the report.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Stats {
    files: usize,
    prod: usize,
    test: usize,
    comments: usize,
    blank: usize,
}

impl Stats {
    fn of_file(c: Counts) -> Stats {
        Stats { files: 1, prod: c.prod, test: c.test, comments: c.comment, blank: c.blank }
    }

    fn add(&mut self, o: &Stats) {
        self.files += o.files;
        self.prod += o.prod;
        self.test += o.test;
        self.comments += o.comments;
        self.blank += o.blank;
    }

    fn lines(&self) -> usize {
        self.prod + self.test + self.comments + self.blank
    }

    fn code(&self) -> usize {
        self.prod + self.test
    }
}

fn sum<'a>(stats: impl IntoIterator<Item = &'a Stats>) -> Stats {
    let mut total = Stats::default();
    for s in stats {
        total.add(s);
    }
    total
}

/// `part` of `whole` as a whole percent, `–` when there is no whole.
fn percent(part: usize, whole: usize) -> String {
    (part * 100 + whole / 2).checked_div(whole).map_or("–".to_string(), |p| format!("{p}%"))
}

// ---- reading the tree ----

fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

fn read(root: &Path, rel: &str) -> Result<String, String> {
    fs::read_to_string(root.join(rel)).map_err(|e| format!("cannot read {rel}: {e}"))
}

/// The files below `dir` (relative to `root`) with the extension `ext`, sorted, as `/` paths. A
/// missing folder has none.
fn list_files(root: &Path, dir: &str, ext: &str) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(root.join(dir)) else { return out };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let rel = join(dir, &name);
        if entry.path().is_dir() {
            if !["target", ".git", "node_modules"].contains(&name.as_str()) {
                out.extend(list_files(root, &rel, ext));
            }
        } else if name.rsplit_once('.').is_some_and(|(_, e)| e == ext) {
            out.push(rel);
        }
    }
    out.sort();
    out
}

/// The file of `mod name;` declared in `parent`, and whether that file is a `mod.rs`-like file
/// (its own `mod y;` declarations are then looked up next to it). Rust looks for `name.rs` and
/// `name/mod.rs` below the folder of the declaring file (below `parent_stem/` when the parent is
/// no `mod.rs`); `#[path]` is relative to the declaring file's folder.
fn find_module(root: &Path, parent: &str, parent_mod_rs: bool, decl: &ModDecl) -> Result<(String, bool), String> {
    let (dir, file) = parent.rsplit_once('/').unwrap_or(("", parent));
    if let Some(path) = &decl.path {
        let rel = join(dir, path);
        return if root.join(&rel).is_file() { Ok((rel, true)) } else { Err(format!("{parent}: #[path = \"{path}\"] of `mod {}` names no file ({rel})", decl.name)) };
    }
    let base = if parent_mod_rs { dir.to_string() } else { join(dir, file.trim_end_matches(".rs")) };
    let plain = join(&base, &format!("{}.rs", decl.name));
    let nested = join(&base, &format!("{}/mod.rs", decl.name));
    if root.join(&plain).is_file() {
        Ok((plain, false))
    } else if root.join(&nested).is_file() {
        Ok((nested, true))
    } else {
        Err(format!("{parent}: `mod {};` has no file (looked for {plain} and {nested})", decl.name))
    }
}

/// One source file reached from `main.rs`.
struct Source {
    rel: String,
    /// The feature it belongs to, or `SHARED`.
    feature: String,
    /// Its top-level module (`main` for `main.rs`).
    module: String,
    counts: Counts,
}

/// Reads `rel` and every file below it, in module order.
fn walk(root: &Path, rel: &str, mod_rs: bool, feature: &str, module: &str, test: bool, out: &mut Vec<Source>) -> Result<(), String> {
    if out.iter().any(|s| s.rel == rel) {
        return Ok(());
    }
    let (counts, decls) = analyze(&read(root, rel)?, test);
    out.push(Source { rel: rel.to_string(), feature: feature.to_string(), module: module.to_string(), counts });
    for decl in decls {
        let (child, child_mod_rs) = find_module(root, rel, mod_rs, &decl)?;
        walk(root, &child, child_mod_rs, feature, module, test || decl.cfg_test, out)?;
    }
    Ok(())
}

/// The feature a top-level module belongs to: itself, or for a `…_tests` module declared under
/// `#[cfg(test)]` the longest production module that is a prefix of its name before `_tests` (on
/// `_` boundaries), else `SHARED`.
fn owner(decl: &ModDecl, top: &[ModDecl]) -> String {
    if !decl.cfg_test {
        return decl.name.clone();
    }
    let mut cand = decl.name.strip_suffix("_tests");
    while let Some(c) = cand {
        if top.iter().any(|d| !d.cfg_test && d.name == c) {
            return c.to_string();
        }
        cand = c.rfind('_').map(|p| &c[..p]);
    }
    SHARED.to_string()
}

/// What the scan of `src/` found.
#[derive(Debug, Default, PartialEq)]
struct SrcScan {
    features: BTreeMap<String, Stats>,
    /// The modules of `SHARED`.
    shared: BTreeMap<String, Stats>,
    /// `.rs` files of `src/` that no `mod` declaration reaches.
    unreached: Vec<String>,
}

fn scan_src(root: &Path) -> Result<SrcScan, String> {
    let mut files = Vec::new();
    let (counts, top) = analyze(&read(root, "src/main.rs")?, false);
    files.push(Source { rel: "src/main.rs".into(), feature: "main".into(), module: "main".into(), counts });
    for decl in &top {
        let (child, mod_rs) = find_module(root, "src/main.rs", true, decl)?;
        walk(root, &child, mod_rs, &owner(decl, &top), &decl.name, decl.cfg_test, &mut files)?;
    }
    let mut scan = SrcScan::default();
    for f in &files {
        let map = if f.feature == SHARED { &mut scan.shared } else { &mut scan.features };
        let key = if f.feature == SHARED { &f.module } else { &f.feature };
        map.entry(key.clone()).or_default().add(&Stats::of_file(f.counts));
    }
    let reached: BTreeSet<&str> = files.iter().map(|f| f.rel.as_str()).collect();
    scan.unreached = list_files(root, "src", "rs").into_iter().filter(|f| !reached.contains(f.as_str())).collect();
    Ok(scan)
}

/// `tests/**/*.rs` as test code, one row per `tests/x.rs` or `tests/x/` (named `x`).
fn scan_integration(root: &Path) -> Result<BTreeMap<String, Stats>, String> {
    let mut rows: BTreeMap<String, Stats> = BTreeMap::new();
    for rel in list_files(root, "tests", "rs") {
        let name = rel["tests/".len()..].split('/').next().unwrap_or_default().trim_end_matches(".rs").to_string();
        rows.entry(name).or_default().add(&Stats::of_file(analyze(&read(root, &rel)?, true).0));
    }
    Ok(rows)
}

/// Shell scripts as code (`prod`), one row per folder.
fn scan_tooling(root: &Path) -> Result<BTreeMap<String, Stats>, String> {
    let mut rows: BTreeMap<String, Stats> = BTreeMap::new();
    for dir in ["scripts", ".forgejo/scripts", "tests", "tools"] {
        for rel in list_files(root, dir, "sh") {
            let folder = rel.rsplit_once('/').map_or("", |(d, _)| d).to_string();
            rows.entry(folder).or_default().add(&Stats::of_file(analyze_shell(&read(root, &rel)?)));
        }
    }
    Ok(rows)
}

fn git_commit(root: &Path) -> Option<String> {
    let out = Command::new("git").args(["rev-parse", "--short", "HEAD"]).current_dir(root).output().ok()?;
    let commit = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (out.status.success() && !commit.is_empty()).then_some(commit)
}

// ---- featuresets ----

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SetDef {
    name: String,
    features: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    #[serde(default)]
    set: Vec<SetDef>,
}

/// A featureset with its features, biggest first.
#[derive(Debug, PartialEq)]
struct SetRow {
    name: String,
    features: Vec<(String, Stats)>,
    total: Stats,
}

/// Everything the report shows, so rendering needs no file system.
#[derive(Debug, PartialEq)]
struct Report {
    commit: Option<String>,
    /// Biggest first.
    sets: Vec<SetRow>,
    /// Features no set lists.
    other: Vec<(String, Stats)>,
    shared: Vec<(String, Stats)>,
    integration: Vec<(String, Stats)>,
    tooling: Vec<(String, Stats)>,
    unreached: Vec<String>,
}

/// Rows biggest (production plus test code) first, then by name.
fn by_size(rows: impl IntoIterator<Item = (String, Stats)>) -> Vec<(String, Stats)> {
    let mut rows: Vec<_> = rows.into_iter().collect();
    rows.sort_by(|a, b| b.1.code().cmp(&a.1.code()).then_with(|| a.0.cmp(&b.0)));
    rows
}

fn build_report(scan: SrcScan, integration: BTreeMap<String, Stats>, tooling: BTreeMap<String, Stats>, config: &Config, commit: Option<String>) -> Result<Report, String> {
    let mut listed: BTreeMap<&str, &str> = BTreeMap::new();
    let mut sets = Vec::new();
    for set in &config.set {
        let mut features = Vec::new();
        for name in &set.features {
            let Some(stats) = scan.features.get(name) else {
                return Err(format!("the feature `{name}` of the set `{}` is no top-level module of src/ (a feature is a module of src/main.rs that is not only for tests)", set.name));
            };
            if let Some(first) = listed.insert(name, &set.name) {
                return Err(format!("the feature `{name}` is listed twice: in the sets `{first}` and `{}`", set.name));
            }
            features.push((name.clone(), *stats));
        }
        let total = sum(features.iter().map(|(_, s)| s));
        sets.push(SetRow { name: set.name.clone(), features: by_size(features), total });
    }
    sets.sort_by(|a, b| b.total.code().cmp(&a.total.code()).then_with(|| a.name.cmp(&b.name)));
    let other = scan.features.iter().filter(|(name, _)| !listed.contains_key(name.as_str())).map(|(n, s)| (n.clone(), *s));
    Ok(Report { commit, sets, other: by_size(other), shared: by_size(scan.shared), integration: by_size(integration), tooling: by_size(tooling), unreached: scan.unreached })
}

// ---- rendering ----

const COLUMNS: &str = "| Files | Production | Tests | Comments | Blank | Total | Test share |\n|---|---:|---:|---:|---:|---:|---:|---:|\n";

fn table_head(first: &str) -> String {
    format!("| {first} {COLUMNS}")
}

fn row(label: &str, s: &Stats, bold: bool) -> String {
    let cells = [s.files.to_string(), s.prod.to_string(), s.test.to_string(), s.comments.to_string(), s.blank.to_string(), s.lines().to_string(), percent(s.test, s.code())];
    let (label, cells) = if bold { (format!("**{label}**"), cells.map(|c| format!("**{c}**"))) } else { (label.to_string(), cells) };
    format!("| {label} | {} |\n", cells.join(" | "))
}

fn table(first: &str, rows: &[(String, Stats)], total: Option<&str>) -> String {
    let mut out = table_head(first);
    for (name, stats) in rows {
        out += &row(name, stats, false);
    }
    if let Some(label) = total {
        out += &row(label, &sum(rows.iter().map(|(_, s)| s)), true);
    }
    out
}

/// A table of files that are all code or all test (integration tests, scripts): no test columns.
fn plain_table(first: &str, rows: &[(String, Stats)], test: bool) -> String {
    let code = |s: &Stats| if test { s.test } else { s.prod };
    let line = |label: &str, s: &Stats, bold: bool| {
        let cells = [s.files.to_string(), code(s).to_string(), s.comments.to_string(), s.blank.to_string(), s.lines().to_string()];
        if bold {
            format!("| **{label}** | {} |\n", cells.map(|c| format!("**{c}**")).join(" | "))
        } else {
            format!("| {label} | {} |\n", cells.join(" | "))
        }
    };
    let mut out = format!("| {first} | Files | Code | Comments | Blank | Total |\n|---|---:|---:|---:|---:|---:|\n");
    for (name, stats) in rows {
        out += &line(name, stats, false);
    }
    out + &line("Total", &sum(rows.iter().map(|(_, s)| s)), true)
}

fn render(r: &Report) -> String {
    let all_src: Vec<Stats> = r.sets.iter().map(|s| s.total).chain(r.other.iter().map(|(_, s)| *s)).chain(r.shared.iter().map(|(_, s)| *s)).collect();
    let src = sum(&all_src);
    let integration = sum(r.integration.iter().map(|(_, s)| s));
    let tooling = sum(r.tooling.iter().map(|(_, s)| s));
    let comments = src.comments + integration.comments + tooling.comments;
    let blank = src.blank + integration.blank + tooling.blank;
    let total = src.lines() + integration.lines() + tooling.lines();
    let tests = src.test + integration.test;

    let mut out = String::from("# Lines of code\n\nGenerated by `cargo run --example loc_report` (DEV-044)");
    if let Some(commit) = &r.commit {
        let _ = write!(out, " at commit `{commit}`");
    }
    out += ". A line is blank, a comment (`//`, `///`, `//!`, `/* … */`) or code; code is test code inside a module declared under `#[cfg(test)]` or an item marked `#[cfg(test)]`, else production code. The test share is test code ÷ (production code + test code).\n\n";

    out += "## Summary\n\n| | Lines |\n|---|---:|\n";
    for (label, n) in
        [("Production code", src.prod), ("Test code (unit tests, in `src/`)", src.test), ("Integration tests (`tests/`)", integration.test), ("Tooling (shell scripts)", tooling.prod), ("Comments", comments), ("Blank lines", blank)]
    {
        let _ = writeln!(out, "| {label} | {n} |");
    }
    let _ = writeln!(out, "| **Total lines** | **{total}** |");
    let _ = writeln!(out, "| Test share (unit and integration tests ÷ all Rust code) | {} |\n", percent(tests, src.prod + tests));

    out += "## Featuresets\n\nThe sets of `tools/loc-report/featuresets.toml`, biggest first, then the code no set lists and the test modules that belong to no feature.\n\n";
    let mut rows: Vec<(String, Stats)> = r.sets.iter().map(|s| (s.name.clone(), s.total)).collect();
    if !r.other.is_empty() {
        rows.push(("Other".to_string(), sum(r.other.iter().map(|(_, s)| s))));
    }
    if !r.shared.is_empty() {
        rows.push((SHARED.to_string(), sum(r.shared.iter().map(|(_, s)| s))));
    }
    out += &table("Featureset", &rows, Some("Total"));

    out += "\n## Features by featureset\n";
    for set in &r.sets {
        let _ = write!(out, "\n### {}\n\n{}", set.name, table("Feature", &set.features, Some("Total")));
    }
    if !r.shared.is_empty() {
        let _ = write!(out, "\n## {SHARED}\n\nTest modules of no feature (`ui_test_support`, a `…_tests` module without a module of that name), by top-level module.\n\n{}", table("Module", &r.shared, Some("Total")));
    }
    if !r.other.is_empty() {
        let names: Vec<String> = r.other.iter().map(|(n, _)| format!("`{n}`")).collect();
        let _ = write!(out, "\n## Other\n\nThese features are in no set of `tools/loc-report/featuresets.toml`: add {} to a `[[set]]`.\n\n{}", names.join(", "), table("Feature", &r.other, Some("Total")));
    }
    let _ = write!(out, "\n## Integration tests\n\n`tests/x.rs` and `tests/x/` are one row, `x`. All of it is test code.\n\n{}", plain_table("Test", &r.integration, true));
    let _ = write!(out, "\n## Tooling\n\nShell scripts of `scripts/`, `.forgejo/scripts/`, `tests/` and `tools/`, by folder; `#` lines are comments.\n\n{}", plain_table("Folder", &r.tooling, false));
    if !r.unreached.is_empty() {
        let _ = write!(out, "\n## Not reached from main.rs\n\nThese `.rs` files of `src/` are declared by no `mod` and are not counted:\n\n");
        for f in &r.unreached {
            let _ = writeln!(out, "- `{f}`");
        }
    }
    out
}

// ---- the program ----

#[derive(Debug, PartialEq)]
struct Args {
    root: PathBuf,
    config: PathBuf,
    out: Option<PathBuf>,
}

fn parse_args(args: &[String], default_root: &Path) -> Result<Args, String> {
    let (mut root, mut config, mut out) = (None, None, None);
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let slot = match arg.as_str() {
            "--root" => &mut root,
            "--config" => &mut config,
            "--out" => &mut out,
            _ => return Err(format!("unknown argument `{arg}`\n{USAGE}")),
        };
        *slot = Some(PathBuf::from(it.next().ok_or_else(|| format!("{arg} needs a value\n{USAGE}"))?));
    }
    let root = root.unwrap_or_else(|| default_root.to_path_buf());
    let config = config.unwrap_or_else(|| root.join("tools/loc-report/featuresets.toml"));
    Ok(Args { root, config, out })
}

fn generate(args: &Args) -> Result<String, String> {
    let text = fs::read_to_string(&args.config).map_err(|e| format!("cannot read the config {}: {e}", args.config.display()))?;
    let config: Config = toml::from_str(&text).map_err(|e| format!("the config {} is wrong: {e}", args.config.display()))?;
    let report = build_report(scan_src(&args.root)?, scan_integration(&args.root)?, scan_tooling(&args.root)?, &config, git_commit(&args.root))?;
    Ok(render(&report))
}

fn execute(args: &[String], default_root: &Path) -> Result<(), String> {
    let args = parse_args(args, default_root)?;
    let report = generate(&args)?;
    match &args.out {
        Some(path) => fs::write(path, report).map_err(|e| format!("cannot write {}: {e}", path.display())),
        None => {
            print!("{report}");
            Ok(())
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match execute(&args, Path::new(env!("CARGO_MANIFEST_DIR"))) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("loc_report: {message}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A temporary folder with files, removed when dropped.
    struct Tmp(PathBuf);

    impl Tmp {
        fn new(files: &[(&str, &str)]) -> Tmp {
            static N: AtomicUsize = AtomicUsize::new(0);
            let dir = std::env::temp_dir().join(format!("loc_report_{}_{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
            let tmp = Tmp(dir);
            for (rel, text) in files {
                let path = tmp.0.join(rel);
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(path, text).unwrap();
            }
            tmp
        }
    }

    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn counts(text: &str) -> Counts {
        analyze(text, false).0
    }

    fn c(blank: usize, comment: usize, prod: usize, test: usize) -> Counts {
        Counts { blank, comment, prod, test }
    }

    // spec: DEV-044
    #[test]
    fn dev_044_strings_hold_code_even_with_comment_markers_and_braces() {
        let text = "let a = \"// not a comment { \";\nlet b = \"say \\\"hi\\\" /* no\";\nlet c = b\"//\";\n";
        assert_eq!(counts(text), c(0, 0, 3, 0));
        let lexed = lex(text);
        assert!(lexed.lines.iter().take(3).all(|l| l.code && !l.comment));
    }

    // spec: DEV-044
    #[test]
    fn dev_044_raw_strings_end_at_their_hashes() {
        let text = "let a = r\"no // here\";\nlet b = r##\"a \"# still // inside\n\n  /* open\n\"##;\nlet c = br#\"x\"#;\n// done\n";
        // The blank-looking line inside the string is code; the line after the string is code.
        assert_eq!(counts(text), c(0, 1, 6, 0));
        let r#type = 1; // a raw identifier is no raw string
        assert_eq!(counts("let r#type = 1;\n"), c(0, 0, 1, 0));
        assert_eq!(r#type, 1);
    }

    // spec: DEV-044
    #[test]
    fn dev_044_block_comments_nest_and_count_every_line_as_comment() {
        let text = "/* a\n  /* nested */ still comment\n\n*/ let x = 1;\n/* one line */\nfn f() {} /* trailing */\n";
        // Lines: comment, comment, blank, code (`*/ let`), comment, code.
        assert_eq!(counts(text), c(1, 3, 2, 0));
    }

    // spec: DEV-044
    #[test]
    fn dev_044_a_line_with_code_and_a_trailing_comment_is_code() {
        let text = "// header\n\n//! inner doc\n/// doc\nlet x = 1; // why\n  \nlet y = 2; /* b */\n";
        assert_eq!(counts(text), c(2, 3, 2, 0));
    }

    // spec: DEV-044
    #[test]
    fn dev_044_a_char_is_told_from_a_lifetime() {
        let text = "fn f<'a>(x: &'a str) -> &'static str { let q = '\"'; let n = '\\n'; let e = '\\''; let u = '\\u{1F600}'; let z = 'é'; let a = 'a'; \"{\" }\n// c\n";
        assert_eq!(counts(text), c(0, 1, 1, 0));
        // A quote in a char does not open a string that would swallow the comment line.
        let lexed = lex("let q = '\"';\n// still a comment\n");
        assert!(!lexed.lines[1].code && lexed.lines[1].comment);
        // An escaped backslash ends its char at the next quote, not one later.
        let lexed = lex("let b = '\\\\'; let s = \"x\";\n// c\n");
        assert!(lexed.lines[0].code && !lexed.lines[1].code && lexed.lines[1].comment);
        assert_eq!(lexed.tokens.iter().filter(|t| matches!(t.kind, Tk::Str(_))).count(), 1);
        // A lifetime does not open a char that would swallow the next line.
        let lexed = lex("fn f<'a>() {}\nlet s = \"x\"; // end\n");
        assert!(lexed.lines[0].code && lexed.lines[1].code);
    }

    // spec: DEV-044
    #[test]
    fn dev_044_a_cfg_test_module_in_a_production_file_is_test_code() {
        let text = "fn prod() {}\n\n#[cfg(test)]\nmod tests {\n    use super::*;\n    // a note\n\n    #[test]\n    fn t() { assert!(\"}\".len() == 1); }\n}\n\nfn prod2() {}\n";
        // Test: attribute, mod line, use, t's attribute, fn, closing brace = 6 code lines; the
        // comment and the blank line inside count as comment and blank.
        assert_eq!(counts(text), c(3, 1, 2, 6));
    }

    // spec: DEV-044
    #[test]
    fn dev_044_cfg_test_items_of_every_kind_end_where_the_item_ends() {
        let text = "\
#[cfg(test)]
use std::fmt::{self,
    Debug};
fn p1() {}
#[cfg(test)]
fn helper() -> Vec<(u8, u8)> {
    let s = \"}}\"; // }
    let c = '}';
    vec![]
}
fn p2() {}
#[cfg(test)]
#[derive(Debug)]
struct S;
fn p3() {}
#[cfg(test)]
impl X {
    fn a() {}
}
const P4: u8 = 1;
#[cfg(test)]
const T: S = S { a: 1 };
fn p5() {}
struct W {
    keep: u8,
    #[cfg(test)]
    pub(crate) fake: Option<u64>,
    also: u8,
}
enum E {
    A,
    #[cfg(test)]
    Gate(std::sync::mpsc::Receiver<()>),
    B,
}
";
        let ranges = test_ranges(&lex(text).tokens);
        assert_eq!(ranges, [(0, 2), (4, 9), (11, 13), (15, 18), (20, 21), (25, 26), (31, 32)]);
        let (n, _) = analyze(text, false);
        assert_eq!((n.test, n.prod), (3 + 6 + 3 + 4 + 2 + 2 + 2, 13));
    }

    // spec: DEV-044
    #[test]
    fn dev_044_cfg_all_test_is_test_code_but_any_and_not_are_production() {
        let text = "#[cfg(all(test, unix))]\nfn a() {}\n#[cfg(any(test, target_os = \"macos\"))]\nfn b() {}\n#[cfg(not(test))]\nfn c() {}\n#[cfg(feature = \"bench\")]\nfn d() {}\n#[cfg(all(unix, windows))]\nfn e() {}\n#[cfg(all(test, feature = \"x\"))]\nfn f() {}\n";
        assert_eq!(counts(text), c(0, 0, 8, 4));
    }

    // spec: DEV-044
    #[test]
    fn dev_044_an_inner_cfg_test_makes_the_whole_file_test_code() {
        assert_eq!(counts("#![cfg(test)]\nfn a() {}\n"), c(0, 0, 0, 2));
        assert_eq!(analyze("fn a() {}\n// c\n\n", true).0, c(1, 1, 0, 1));
    }

    // spec: DEV-044
    #[test]
    fn dev_044_mod_declarations_keep_their_attributes() {
        let text = "#[cfg(test)]\nmod a;\npub mod b;\n#[cfg(test)]\n#[path = \"x.rs\"]\npub(crate) mod c;\n#[cfg(all(test, unix))]\nmod d;\n#[cfg(feature = \"bench\")]\nmod e;\nmod inline { mod hidden; }\nmod f {\n}\nfn g() { }\nmod h;\n";
        let decl = |name: &str, cfg_test: bool, path: Option<&str>| ModDecl { name: name.into(), cfg_test, path: path.map(Into::into) };
        assert_eq!(mod_decls(&lex(text).tokens), [decl("a", true, None), decl("b", false, None), decl("c", true, Some("x.rs")), decl("d", true, None), decl("e", false, None), decl("h", false, None)]);
    }

    fn src_tree() -> Tmp {
        Tmp::new(&[
            ("src/main.rs", "mod alpha;\n#[cfg(test)]\nmod alpha_ui_tests;\nmod beta;\n#[cfg(test)]\nmod support;\n#[cfg(test)]\nmod orphan_tests;\nfn main() {}\n"),
            ("src/alpha.rs", "mod inner;\n#[cfg(test)]\n#[path = \"alpha_check.rs\"]\nmod check;\nfn a() {}\n"),
            ("src/alpha/inner.rs", "fn i() {}\n"),
            ("src/alpha_check.rs", "mod deeper;\nfn c() {}\n"),
            ("src/deeper.rs", "fn never() {}\n"),
            ("src/alpha_ui_tests.rs", "fn t1() {}\nfn t2() {}\n"),
            ("src/beta/mod.rs", "mod sub;\nfn b() {}\n"),
            ("src/beta/sub.rs", "fn s() {}\n\n// c\n"),
            ("src/support.rs", "fn s() {}\n"),
            ("src/orphan_tests.rs", "fn o() {}\n"),
            ("src/stray.rs", "fn stray() {}\n"),
            ("src/testdata/data.md", "not rust\n"),
        ])
    }

    // spec: DEV-044
    #[test]
    fn dev_044_the_module_tree_is_followed_through_folders_and_path_attributes() {
        let tmp = src_tree();
        let scan = scan_src(&tmp.0).unwrap();
        let stats = |f: &str| scan.features[f];
        // main.rs is its own feature.
        assert_eq!((stats("main").files, stats("main").prod, stats("main").test), (1, 3, 6));
        // alpha: alpha.rs, alpha/inner.rs, the test module alpha_check.rs by #[path] (relative to
        // src/, the folder of the declaring file) and its alpha_ui_tests; src/deeper.rs is looked
        // for below alpha_check's folder, as the file is mod.rs-like, so it is reached too.
        let alpha = stats("alpha");
        assert_eq!(alpha.files, 5);
        // alpha.rs: `mod inner;` (1), the #[cfg(test)] #[path] mod check; (3 test), fn a (1);
        // inner 1; alpha_check 2 test; deeper 1 test; alpha_ui_tests 2 test.
        assert_eq!((alpha.prod, alpha.test), (3, 8));
        assert_eq!(stats("beta").files, 2);
        assert_eq!(scan.unreached, ["src/stray.rs"]);
    }

    // spec: DEV-044
    #[test]
    fn dev_044_a_missing_module_file_is_an_error() {
        let tmp = Tmp::new(&[("src/main.rs", "mod gone;\n")]);
        let err = scan_src(&tmp.0).unwrap_err();
        assert!(err.contains("`mod gone;`") && err.contains("src/gone.rs") && err.contains("src/gone/mod.rs"), "{err}");
        let tmp = Tmp::new(&[("src/main.rs", "#[path = \"nope.rs\"]\nmod gone;\n")]);
        assert!(scan_src(&tmp.0).unwrap_err().contains("nope.rs"));
        let tmp = Tmp::new(&[]);
        assert!(scan_src(&tmp.0).unwrap_err().contains("src/main.rs"));
    }

    // spec: DEV-044
    #[test]
    fn dev_044_a_tests_module_belongs_to_the_longest_module_that_is_a_prefix_of_its_name() {
        let decl = |name: &str, cfg_test: bool| ModDecl { name: name.into(), cfg_test, path: None };
        let top = [
            decl("fold", false),
            decl("fold_ui", false),
            decl("fold_ui_tests", true),
            decl("heading_levels", false),
            decl("heading_levels_ui_tests", true),
            decl("dataflow", true),
            decl("dataflow_tests", true),
            decl("cursor_style", false),
            decl("cursor_ui_tests", true),
            decl("ui_test_support", true),
            decl("utf8_tests", true),
            decl("markdown", false),
        ];
        let of = |name: &str| owner(top.iter().find(|d| d.name == name).unwrap(), &top);
        assert_eq!(of("fold_ui_tests"), "fold_ui");
        assert_eq!(of("heading_levels_ui_tests"), "heading_levels");
        assert_eq!(of("fold"), "fold", "a production module is its own feature");
        assert_eq!(of("dataflow"), SHARED, "a test module without `_tests`");
        assert_eq!(of("dataflow_tests"), SHARED, "its owner exists only for tests");
        assert_eq!(of("cursor_ui_tests"), SHARED, "`cursor_ui` and `cursor` are no modules; `cursor_style` is no prefix");
        assert_eq!(of("ui_test_support"), SHARED);
        assert_eq!(of("utf8_tests"), SHARED);
    }

    // spec: DEV-044
    #[test]
    fn dev_044_test_modules_go_to_their_feature_or_to_shared_test_support() {
        let tmp = src_tree();
        let scan = scan_src(&tmp.0).unwrap();
        assert_eq!(scan.features.keys().collect::<Vec<_>>(), ["alpha", "beta", "main"]);
        assert_eq!(scan.shared.keys().collect::<Vec<_>>(), ["orphan_tests", "support"]);
        assert_eq!(scan.shared["support"], Stats { files: 1, prod: 0, test: 1, comments: 0, blank: 0 });
    }

    fn stats(files: usize, prod: usize, test: usize, comments: usize, blank: usize) -> Stats {
        Stats { files, prod, test, comments, blank }
    }

    fn config(toml: &str) -> Config {
        toml::from_str(toml).unwrap()
    }

    fn scan(features: &[(&str, Stats)]) -> SrcScan {
        SrcScan { features: features.iter().map(|(n, s)| (n.to_string(), *s)).collect(), ..SrcScan::default() }
    }

    fn report(scan: SrcScan, toml: &str) -> Result<Report, String> {
        build_report(scan, BTreeMap::new(), BTreeMap::new(), &config(toml), None)
    }

    // spec: DEV-044
    #[test]
    fn dev_044_an_unknown_or_doubly_listed_feature_is_an_error() {
        let s = || scan(&[("mail", stats(1, 10, 0, 0, 0)), ("secret", stats(1, 5, 5, 0, 0))]);
        let err = report(s(), "[[set]]\nname = \"Mail\"\nfeatures = [\"mail\", \"nope\"]\n").unwrap_err();
        assert!(err.contains("`nope`") && err.contains("`Mail`"), "{err}");
        let err = report(s(), "[[set]]\nname = \"A\"\nfeatures = [\"mail\"]\n[[set]]\nname = \"B\"\nfeatures = [\"secret\", \"mail\"]\n").unwrap_err();
        assert!(err.contains("`mail`") && err.contains("`A`") && err.contains("`B`"), "{err}");
        let err = report(s(), "[[set]]\nname = \"A\"\nfeatures = [\"mail\", \"mail\"]\n").unwrap_err();
        assert!(err.contains("`mail`") && err.contains("twice"), "{err}");
        let mut with_shared = s();
        with_shared.shared.insert("ui_test_support".into(), stats(1, 0, 3, 0, 0));
        assert!(report(with_shared, "[[set]]\nname = \"A\"\nfeatures = [\"ui_test_support\"]\n").is_err(), "shared test support is no feature");
    }

    // spec: DEV-044
    #[test]
    fn dev_044_a_feature_no_set_lists_goes_to_other() {
        let r = report(scan(&[("mail", stats(1, 10, 0, 0, 0)), ("zeta", stats(1, 3, 0, 0, 0)), ("alpha", stats(1, 3, 0, 0, 0))]), "[[set]]\nname = \"Mail\"\nfeatures = [\"mail\"]\n").unwrap();
        assert_eq!(r.other.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["alpha", "zeta"]);
        let text = render(&r);
        assert!(text.contains("\n## Other\n") && text.contains("`alpha`, `zeta`") && text.contains("| Other | 2 | 6 |"), "{text}");
        let r = report(scan(&[("mail", stats(1, 10, 0, 0, 0))]), "[[set]]\nname = \"Mail\"\nfeatures = [\"mail\"]\n").unwrap();
        assert!(r.other.is_empty() && !render(&r).contains("## Other"));
    }

    // spec: DEV-044
    #[test]
    fn dev_044_set_totals_are_the_sum_of_their_features_and_everything_is_ordered_by_size() {
        let r = report(
            scan(&[("a", stats(2, 10, 5, 3, 1)), ("b", stats(1, 40, 0, 0, 2)), ("c", stats(3, 1, 1, 1, 1)), ("d", stats(1, 15, 0, 0, 0)), ("e", stats(1, 15, 0, 0, 0))]),
            "[[set]]\nname = \"Small\"\nfeatures = [\"c\"]\n[[set]]\nname = \"Big\"\nfeatures = [\"a\", \"b\", \"e\", \"d\"]\n",
        )
        .unwrap();
        assert_eq!(r.sets.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["Big", "Small"]);
        let big = &r.sets[0];
        assert_eq!(big.features.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["b", "a", "d", "e"], "size, then name");
        assert_eq!(big.total, sum(big.features.iter().map(|(_, s)| s)));
        assert_eq!(big.total, stats(5, 80, 5, 3, 3));
    }

    fn fixed() -> Report {
        Report {
            commit: Some("abc1234".into()),
            sets: vec![
                SetRow { name: "Mail".into(), features: vec![("mail".into(), stats(3, 80, 20, 10, 5)), ("secret".into(), stats(1, 10, 0, 0, 0))], total: stats(4, 90, 20, 10, 5) },
                SetRow { name: "Empty".into(), features: vec![], total: Stats::default() },
            ],
            other: vec![],
            shared: vec![("ui_test_support".into(), stats(1, 0, 30, 2, 3))],
            integration: vec![("cli_start".into(), stats(1, 0, 40, 4, 6))],
            tooling: vec![("scripts".into(), stats(2, 25, 0, 5, 5))],
            unreached: vec!["src/stray.rs".into()],
        }
    }

    // spec: DEV-044
    #[test]
    fn dev_044_the_report_is_rendered_as_markdown_tables() {
        let text = render(&fixed());
        let lines: Vec<&str> = text.lines().collect();
        let has = |l: &str| assert!(lines.contains(&l), "missing line {l:?} in\n{text}");
        has("# Lines of code");
        assert!(lines[2].contains("at commit `abc1234`"), "{}", lines[2]);
        // Summary: prod 90, unit test 20 + 30, integration 40, shell 25, comments 10+2+4+5, blank 5+3+6+5.
        has("| Production code | 90 |");
        has("| Test code (unit tests, in `src/`) | 50 |");
        has("| Integration tests (`tests/`) | 40 |");
        has("| Tooling (shell scripts) | 25 |");
        has("| Comments | 21 |");
        has("| Blank lines | 19 |");
        has("| **Total lines** | **245** |");
        has("| Test share (unit and integration tests ÷ all Rust code) | 50% |");
        has("| Featureset | Files | Production | Tests | Comments | Blank | Total | Test share |");
        has("|---|---:|---:|---:|---:|---:|---:|---:|");
        has("| Mail | 4 | 90 | 20 | 10 | 5 | 125 | 18% |");
        has("| Empty | 0 | 0 | 0 | 0 | 0 | 0 | – |");
        has("| Shared test support | 1 | 0 | 30 | 2 | 3 | 35 | 100% |");
        has("| **Total** | **5** | **90** | **50** | **12** | **8** | **160** | **36%** |");
        has("### Mail");
        has("| mail | 3 | 80 | 20 | 10 | 5 | 115 | 20% |");
        has("| secret | 1 | 10 | 0 | 0 | 0 | 10 | 0% |");
        has("| **Total** | **4** | **90** | **20** | **10** | **5** | **125** | **18%** |");
        has("## Shared test support");
        has("| ui_test_support | 1 | 0 | 30 | 2 | 3 | 35 | 100% |");
        has("| Test | Files | Code | Comments | Blank | Total |");
        has("| cli_start | 1 | 40 | 4 | 6 | 50 |");
        has("| scripts | 2 | 25 | 5 | 5 | 35 |");
        has("| **Total** | **2** | **25** | **5** | **5** | **35** |");
        has("## Not reached from main.rs");
        has("- `src/stray.rs`");
        assert!(!text.contains("## Other"));
        assert_eq!(render(&fixed()), text, "the same data gives the same text");
        let mut no_commit = fixed();
        no_commit.commit = None;
        assert!(!render(&no_commit).contains("at commit"));
    }

    // spec: DEV-044
    #[test]
    fn dev_044_a_test_share_is_rounded_and_a_dash_without_code() {
        assert_eq!(percent(1, 3), "33%");
        assert_eq!(percent(2, 3), "67%");
        assert_eq!(percent(0, 5), "0%");
        assert_eq!(percent(5, 5), "100%");
        assert_eq!(percent(0, 0), "–");
    }

    // spec: DEV-044
    #[test]
    fn dev_044_shell_scripts_count_hash_lines_as_comments_but_not_the_shebang() {
        assert_eq!(analyze_shell("#!/bin/sh\n# note\n\nls # trailing\n  # indented\n"), c(1, 2, 2, 0));
    }

    // spec: DEV-044
    #[test]
    fn dev_044_integration_tests_and_scripts_are_grouped() {
        let tmp = Tmp::new(&[
            ("tests/x.rs", "#[test]\nfn a() {}\n// c\n"),
            ("tests/x/helper.rs", "fn h() {}\n\n"),
            ("tests/y.rs", "fn y() {}\n"),
            ("tests/mail-env/run.sh", "#!/bin/sh\nls\n"),
            ("scripts/a.sh", "#!/bin/sh\n# c\n"),
            (".forgejo/scripts/b.sh", "echo\n"),
            ("tools/loc-report/t.sh", "echo\n"),
            ("scripts/readme.md", "x"),
        ]);
        let integration = scan_integration(&tmp.0).unwrap();
        assert_eq!(integration.keys().collect::<Vec<_>>(), ["x", "y"]);
        assert_eq!(integration["x"], stats(2, 0, 3, 1, 1), "tests/x.rs and tests/x/ are one row");
        let tooling = scan_tooling(&tmp.0).unwrap();
        assert_eq!(tooling.keys().collect::<Vec<_>>(), [".forgejo/scripts", "scripts", "tests/mail-env", "tools/loc-report"]);
        assert_eq!(tooling["scripts"], stats(1, 1, 0, 1, 0));
    }

    // spec: DEV-044
    #[test]
    fn dev_044_the_arguments_are_read_and_a_wrong_one_is_named() {
        let args = |a: &[&str]| parse_args(&a.iter().map(|s| s.to_string()).collect::<Vec<_>>(), Path::new("/r"));
        assert_eq!(args(&[]).unwrap(), Args { root: "/r".into(), config: "/r/tools/loc-report/featuresets.toml".into(), out: None });
        assert_eq!(args(&["--root", "/x", "--out", "o.md"]).unwrap(), Args { root: "/x".into(), config: "/x/tools/loc-report/featuresets.toml".into(), out: Some("o.md".into()) });
        assert_eq!(args(&["--config", "c.toml"]).unwrap().config, Path::new("c.toml"));
        assert!(args(&["--bogus"]).unwrap_err().contains("unknown argument `--bogus`"));
        assert!(args(&["--root"]).unwrap_err().contains("--root needs a value"));
    }

    // spec: DEV-044
    #[test]
    fn dev_044_the_program_writes_the_report_or_names_the_problem() {
        let tmp = Tmp::new(&[
            ("src/main.rs", "mod a;\nfn main() {}\n"),
            ("src/a.rs", "fn a() {}\n#[cfg(test)]\nmod tests {\n}\n"),
            ("cfg.toml", "[[set]]\nname = \"All\"\nfeatures = [\"a\", \"main\"]\n"),
            ("bad.toml", "[[set]]\nname = \"All\"\nfeatures = [\"a\"]\nextra = 1\n"),
        ]);
        let run = |extra: &[&str]| {
            let mut args: Vec<String> = ["--root", tmp.0.to_str().unwrap()].map(String::from).to_vec();
            args.extend(extra.iter().map(|s| s.to_string()));
            execute(&args, Path::new("/unused"))
        };
        let out = tmp.0.join("out.md");
        run(&["--config", tmp.0.join("cfg.toml").to_str().unwrap(), "--out", out.to_str().unwrap()]).unwrap();
        let text = fs::read_to_string(&out).unwrap();
        assert!(text.starts_with("# Lines of code") && text.contains("| All | 2 |") && !text.contains("## Other"), "{text}");
        assert!(run(&["--config", "/nonexistent/cfg.toml"]).unwrap_err().contains("cannot read the config"));
        assert!(run(&["--config", tmp.0.join("bad.toml").to_str().unwrap()]).unwrap_err().contains("is wrong"));
        assert!(run(&["--config", tmp.0.join("cfg.toml").to_str().unwrap(), "--out", "/nonexistent/o.md"]).unwrap_err().contains("cannot write"));
        assert_eq!(git_commit(Path::new("/nonexistent")), None);
    }

    // spec: DEV-044
    #[test]
    fn dev_044_the_report_of_this_repository_is_complete() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let args = parse_args(&[], root).unwrap();
        let text = generate(&args).expect("the committed config fits the code");
        assert!(!text.contains("## Other"), "a feature is in no set of tools/loc-report/featuresets.toml:\n{text}");
        assert!(!text.contains("## Not reached"), "a file of src/ is declared by no module:\n{text}");
        let scan = scan_src(root).unwrap();
        let main = fs::read_to_string(root.join("src/main.rs")).unwrap();
        let (_, top) = analyze(&main, false);
        let production: Vec<&ModDecl> = top.iter().filter(|d| !d.cfg_test).collect();
        assert!(production.len() > 90, "the modules of main.rs are found ({})", production.len());
        for decl in production {
            assert!(scan.features.contains_key(&decl.name), "{} is no feature", decl.name);
            assert!(text.contains(&format!("| {} |", decl.name)), "{} is not in the report", decl.name);
        }
        let all = sum(scan.features.values().chain(scan.shared.values()));
        assert!(all.prod > 50_000 && all.test > 50_000 && all.comments > 0 && all.blank > 0, "{all:?}");
        let sets = build_report(scan, scan_integration(root).unwrap(), scan_tooling(root).unwrap(), &toml::from_str(&fs::read_to_string(&args.config).unwrap()).unwrap(), None).unwrap();
        assert!(sets.sets.len() >= 8 && sets.integration.len() > 10 && !sets.tooling.is_empty());
    }
}
