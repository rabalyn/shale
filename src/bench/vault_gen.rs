//! The synthetic vaults of the benchmarks (BUILD-029): the same notes on every run (a fixed
//! seed), with what real notes have: front matter, headings, lists, tasks, tags, wiki links in
//! all their forms, code blocks, tables, quotes, and a few very long notes.

use std::fmt::Write as _;
use std::io;
use std::path::{Path, PathBuf};

/// How big a vault is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Size {
    Small,
    Large,
}

/// The number of notes of the two sizes.
#[derive(Clone, Copy, Debug)]
pub struct Sizes {
    pub small: usize,
    pub large: usize,
}

impl Sizes {
    /// what the benchmarks run on: about 200 and about 5000 notes
    pub const STANDARD: Sizes = Sizes { small: 200, large: 5000 };

    pub fn notes(&self, size: Size) -> usize {
        match size {
            Size::Small => self.small,
            Size::Large => self.large,
        }
    }
}

/// The size of the very long notes of a vault, in bytes (at least).
pub const LONG_NOTE_BYTES: usize = 200 * 1024;

/// What a generated vault holds that the scenarios look for.
#[derive(Clone, Debug)]
pub struct Generated {
    pub root: PathBuf,
    /// a very long note (about 200 KB), in the root folder
    pub long_note: PathBuf,
    /// the note most others link to (the last of the generated ones; the first, which a window opens, has hardly any links to it)
    pub popular: PathBuf,
    /// a drawing (Excalidraw) with many elements, in the root folder
    pub drawing: PathBuf,
}

/// xorshift64*: small, fast and the same everywhere.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.max(1))
    }

    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// 0..n
    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }

    /// true in `pct` of 100 cases
    pub fn chance(&mut self, pct: usize) -> bool {
        self.below(100) < pct
    }

    /// 0..n, the low numbers far more often (a few notes are linked by many)
    pub fn skewed(&mut self, n: usize) -> usize {
        let r = (self.next() >> 11) as f64 / (1u64 << 53) as f64;
        ((r * r * r * n as f64) as usize).min(n.saturating_sub(1))
    }

    fn pick<'a>(&mut self, words: &[&'a str]) -> &'a str {
        words[self.below(words.len())]
    }
}

const WORDS: &[&str] = &[
    "garden",
    "planning",
    "river",
    "meeting",
    "recipe",
    "budget",
    "project",
    "reading",
    "travel",
    "idea",
    "design",
    "history",
    "kitchen",
    "winter",
    "summer",
    "history",
    "network",
    "library",
    "system",
    "memory",
    "problem",
    "answer",
    "season",
    "journal",
    "morning",
    "evening",
    "weekend",
    "review",
    "draft",
    "outline",
    "notes",
    "summary",
    "research",
    "paper",
    "chapter",
    "theory",
    "method",
    "result",
    "question",
    "plan",
    "goal",
    "habit",
    "health",
    "music",
    "guitar",
    "piano",
    "photo",
    "camera",
    "forest",
    "mountain",
    "ocean",
    "island",
    "market",
    "coffee",
    "bread",
    "soup",
    "pasta",
    "salad",
    "tool",
    "machine",
    "engine",
    "code",
    "server",
    "client",
    "browser",
    "window",
    "screen",
    "keyboard",
    "cable",
    "battery",
    "solar",
    "energy",
    "water",
    "stone",
    "metal",
    "paper",
    "glass",
    "wood",
    "light",
    "shadow",
    "colour",
    "sound",
    "silence",
    "story",
    "poem",
    "letter",
    "message",
    "contract",
    "invoice",
    "report",
    "customer",
    "supplier",
    "schedule",
    "deadline",
    "milestone",
    "backlog",
    "release",
    "feature",
    "bug",
    "test",
    "deploy",
    "cluster",
    "database",
    "index",
    "cache",
    "queue",
    "thread",
    "signal",
    "pattern",
    "lesson",
];
const SENTENCE_STARTS: &[&str] = &["Remember that", "It turns out that", "The main point is that", "Another thing:", "Worth checking again:", "Compared to last time,", "In short,", "Later on,", "For now,", "As a rule,"];
const CODE: &[&str] = &[
    "```rust\nfn total(items: &[u32]) -> u32 {\n    items.iter().sum()\n}\n\nfn main() {\n    println!(\"{}\", total(&[1, 2, 3]));\n}\n```",
    "```python\ndef fib(n):\n    a, b = 0, 1\n    for _ in range(n):\n        a, b = b, a + b\n    return a\n```",
    "```sh\nfor f in *.md; do\n  wc -l \"$f\"\ndone | sort -n | tail -5\n```",
    "```json\n{\"name\": \"example\", \"items\": [1, 2, 3], \"nested\": {\"ok\": true}}\n```",
];

fn sentence(r: &mut Rng, titles: &[String], out: &mut String) {
    if r.chance(25) {
        out.push_str(r.pick(SENTENCE_STARTS));
        out.push(' ');
    }
    for i in 0..4 + r.below(9) {
        if i > 0 {
            out.push(' ');
        }
        match r.below(40) {
            0 => {
                let _ = write!(out, "**{}**", r.pick(WORDS));
            }
            1 => {
                let _ = write!(out, "*{}*", r.pick(WORDS));
            }
            2 => {
                let _ = write!(out, "`{}`", r.pick(WORDS));
            }
            3 | 4 => {
                // a wiki link: plain, with an alias, to a heading, or to a note that does not exist
                // the notes at the end of the list are the ones many link to, so the first note of the vault (the one a window opens) is not
                let t = &titles[titles.len() - 1 - r.skewed(titles.len())];
                match r.below(10) {
                    0 => {
                        let _ = write!(out, "[[{t}|{}]]", r.pick(WORDS));
                    }
                    1 => {
                        let _ = write!(out, "[[{t}#Details]]");
                    }
                    2 => {
                        let _ = write!(out, "[[Missing {} {}]]", r.pick(WORDS), r.below(50));
                    }
                    _ => {
                        let _ = write!(out, "[[{t}]]");
                    }
                }
            }
            5 => {
                let _ = write!(out, "#{}", r.pick(WORDS));
            }
            6 => {
                let _ = write!(out, "[{}](https://example.com/{})", r.pick(WORDS), r.pick(WORDS));
            }
            _ => out.push_str(r.pick(WORDS)),
        }
    }
    out.push('.');
}

fn paragraph(r: &mut Rng, titles: &[String], out: &mut String) {
    for i in 0..2 + r.below(4) {
        if i > 0 {
            out.push(' ');
        }
        sentence(r, titles, out);
    }
    out.push_str("\n\n");
}

/// One block of a note: a section with a heading and some of the things notes hold.
fn section(r: &mut Rng, titles: &[String], out: &mut String) {
    let _ = writeln!(out, "## {} {}\n", r.pick(WORDS), r.pick(WORDS));
    paragraph(r, titles, out);
    match r.below(10) {
        0..=2 => {
            for _ in 0..3 + r.below(4) {
                out.push_str("- ");
                sentence(r, titles, out);
                out.push('\n');
            }
            out.push('\n');
        }
        3..=4 => {
            for _ in 0..2 + r.below(4) {
                let _ = write!(out, "- [{}] ", if r.chance(40) { 'x' } else { ' ' });
                sentence(r, titles, out);
                out.push('\n');
            }
            out.push('\n');
        }
        5 => {
            for i in 1..=3 + r.below(3) {
                let _ = write!(out, "{i}. ");
                sentence(r, titles, out);
                out.push('\n');
            }
            out.push('\n');
        }
        6 => {
            out.push_str(CODE[r.below(CODE.len())]);
            out.push_str("\n\n");
        }
        7 => {
            out.push_str("| Item | Count | Note |\n|---|---:|---|\n");
            for _ in 0..2 + r.below(5) {
                let _ = writeln!(out, "| {} | {} | {} |", r.pick(WORDS), r.below(500), r.pick(WORDS));
            }
            out.push('\n');
        }
        8 => {
            out.push_str("> ");
            sentence(r, titles, out);
            out.push_str("\n\n");
        }
        _ => {}
    }
    if r.chance(30) {
        paragraph(r, titles, out);
    }
}

fn note_text(r: &mut Rng, title: &str, titles: &[String], min_bytes: usize) -> String {
    let mut out = String::new();
    if r.chance(30) {
        let _ = write!(out, "---\ntags: [{}, {}]\ncreated: 2026-0{}-{:02}\n---\n\n", r.pick(WORDS), r.pick(WORDS), 1 + r.below(9), 1 + r.below(28));
    }
    let _ = write!(out, "# {title}\n\n");
    paragraph(r, titles, &mut out);
    for _ in 0..3 + r.below(5) {
        section(r, titles, &mut out);
    }
    while out.len() < min_bytes {
        section(r, titles, &mut out);
    }
    out
}

/// The title of note `i`: unique, so every wiki link has one target.
fn title(i: usize, r: &mut Rng) -> String {
    format!("Note {i:04} {} {}", r.pick(WORDS), r.pick(WORDS))
}

/// Write `notes` notes (and the long notes) into `root`, spread over folders; the same files for
/// the same arguments.
pub fn generate(root: &Path, notes: usize, long_notes: usize, drawing_elements: usize) -> io::Result<Generated> {
    let mut r = Rng::new(0x5EED_0343);
    let folders = (notes / 100).clamp(4, 50);
    let titles: Vec<String> = (0..notes).map(|i| title(i, &mut r)).collect();
    for f in 0..folders {
        std::fs::create_dir_all(root.join(format!("Area {f:02}")))?;
    }
    let mut popular = PathBuf::new();
    for (i, t) in titles.iter().enumerate() {
        let rel = PathBuf::from(format!("Area {:02}", i % folders)).join(format!("{t}.md"));
        if i == notes - 1 {
            popular = rel.clone();
        }
        std::fs::write(root.join(&rel), note_text(&mut r, t, &titles, 0))?;
    }
    let mut long_note = PathBuf::new();
    for i in 0..long_notes.max(1) {
        let name = format!("Long note {}", i + 1);
        let rel = PathBuf::from(format!("{name}.md"));
        if i == 0 {
            long_note = rel.clone();
        }
        std::fs::write(root.join(&rel), note_text(&mut r, &name, &titles, LONG_NOTE_BYTES))?;
    }
    let drawing = PathBuf::from("Drawing 1.excalidraw");
    std::fs::write(root.join(&drawing), super::content_gen::drawing(7, drawing_elements))?;
    Ok(Generated { root: root.to_path_buf(), long_note, popular, drawing })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("shale-bench-gen-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn all_files(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
        let mut out = vec![];
        let mut stack = vec![root.to_path_buf()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(d).unwrap() {
                let p = e.unwrap().path();
                if p.is_dir() {
                    stack.push(p);
                } else {
                    out.push((p.strip_prefix(root).unwrap().to_path_buf(), std::fs::read(&p).unwrap()));
                }
            }
        }
        out.sort();
        out
    }

    // spec: BUILD-029
    #[test]
    fn the_same_vault_comes_out_every_time() {
        let (a, b) = (tmp("a"), tmp("b"));
        generate(&a, 40, 1, 20).unwrap();
        generate(&b, 40, 1, 20).unwrap();
        assert_eq!(all_files(&a), all_files(&b));
        let (_, one) = &all_files(&a)[0];
        assert!(!one.is_empty());
        let _ = (std::fs::remove_dir_all(&a), std::fs::remove_dir_all(&b));
    }

    // spec: BUILD-029
    #[test]
    fn the_vault_has_the_notes_the_long_note_and_what_notes_hold() {
        let d = tmp("c");
        let g = generate(&d, 120, 2, 20).unwrap();
        let files = all_files(&d);
        assert_eq!(files.len(), 123, "the notes, the two long ones and the drawing");
        let long = std::fs::read_to_string(d.join(&g.long_note)).unwrap();
        assert!(long.len() >= LONG_NOTE_BYTES, "{} bytes", long.len());
        assert!(d.join(&g.popular).is_file());
        let all: String = files.iter().map(|(_, b)| String::from_utf8_lossy(b).into_owned()).collect();
        for needle in ["\n## ", "\n- [ ] ", "\n- [x] ", "\n1. ", "```rust", "|---|", "\n> ", "[[Note ", "|", "#Details]]", "[[Missing ", " #", "tags: [", "](https://"] {
            assert!(all.contains(needle), "{needle:?} is missing");
        }
        // the vault reads as a vault: the links resolve
        let v = crate::vault::Vault::open(&d).unwrap();
        assert_eq!(v.notes.len(), 122);
        assert_eq!(v.drawings.len(), 1);
        let popular = v.backlinks(&g.popular);
        assert!(popular.len() > 10, "the popular note has many backlinks, {}", popular.len());
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: BUILD-029
    #[test]
    fn the_standard_sizes_are_200_and_5000_notes() {
        assert_eq!((Sizes::STANDARD.notes(Size::Small), Sizes::STANDARD.notes(Size::Large)), (200, 5000));
    }
}
