//! Vim-lite editing for notes (setting "Vim-style editing"): normal, insert, visual and
//! line-wise visual mode.
//!
//! Everything here works on the note text and a cursor given as a **character** index, with no
//! UI, so it can be tested directly. The editor (`editor.rs`) feeds the keys of a frame in
//! (typed characters, Esc, Enter, Backspace, arrows), applies the cursor or selection to the text
//! box and carries out the effects (clipboard, paste request).
//!
//! The cursor in normal mode sits *on* a character, as in vim: never on a line break, except on
//! an empty line.

use crate::markdown;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Insert,
    Visual,
    VisualLine,
}

/// One key as vim-lite sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VimKey {
    Char(char),
    Esc,
    Enter,
    Backspace,
    Left,
    Right,
    Up,
    Down,
}

/// What the editor has to do after the keys of a frame.
#[derive(Default, Debug, PartialEq)]
pub struct Effects {
    /// the text was changed
    pub changed: bool,
    /// put this on the system clipboard
    pub copy: Option<String>,
    /// ask the window for the system clipboard (it arrives as a paste event; then call `paste`)
    pub request_paste: bool,
    /// the cursor moved: scroll it into view
    pub moved: bool,
}

#[derive(Clone, Debug)]
pub struct Vim {
    pub mode: Mode,
    /// the vim cursor (char index); in visual mode the moving end of the selection
    pub cursor: usize,
    /// where visual mode started (char index)
    anchor: usize,
    /// column that j / k keep (char offset in the line)
    want_col: Option<usize>,
    /// a `g` waiting for the second `g`
    pending_g: bool,
    /// a command being typed: `:` or `/`, and its text
    cmdline: Option<(char, String)>,
    last_search: Option<String>,
    /// a short message for the status bar ("Pattern not found: …")
    pub message: Option<String>,
    /// `p` asked for the clipboard; the next paste event is the text to put in
    pub paste_pending: bool,
}

impl Default for Vim {
    fn default() -> Self {
        Vim { mode: Mode::Normal, cursor: 0, anchor: 0, want_col: None, pending_g: false, cmdline: None, last_search: None, message: None, paste_pending: false }
    }
}

// ---------------------------------------------------------------------------------------------
// Lines and words on a char slice

fn line_start(c: &[char], pos: usize) -> usize {
    let pos = pos.min(c.len());
    c[..pos].iter().rposition(|&x| x == '\n').map(|i| i + 1).unwrap_or(0)
}

/// Index of the line break ending the line (or the text length).
fn line_end(c: &[char], pos: usize) -> usize {
    let pos = pos.min(c.len());
    c[pos..].iter().position(|&x| x == '\n').map(|i| pos + i).unwrap_or(c.len())
}

/// The last character of the line (the line start on an empty line).
fn last_on_line(c: &[char], pos: usize) -> usize {
    let (s, e) = (line_start(c, pos), line_end(c, pos));
    if e > s {
        e - 1
    } else {
        s
    }
}

fn first_non_blank(c: &[char], pos: usize) -> usize {
    let (s, e) = (line_start(c, pos), line_end(c, pos));
    (s..e).find(|&i| !c[i].is_whitespace()).unwrap_or(s)
}

/// Where the cursor may stand in normal mode: on a character of its line, not on the break.
fn clamp_normal(c: &[char], pos: usize) -> usize {
    let pos = pos.min(c.len());
    pos.min(last_on_line(c, pos))
}

/// 0 = blank, 1 = word character, 2 = other symbol (for `w`/`b`); for WORDs only blank or not.
fn class(ch: char, big: bool) -> u8 {
    if ch.is_whitespace() {
        0
    } else if big || ch.is_alphanumeric() || ch == '_' {
        1
    } else {
        2
    }
}

fn word_forward(c: &[char], pos: usize, big: bool) -> usize {
    if c.is_empty() {
        return 0;
    }
    let mut i = pos;
    let k = class(c[i.min(c.len() - 1)], big);
    if k != 0 {
        while i < c.len() && class(c[i], big) == k {
            i += 1;
        }
    }
    while i < c.len() && class(c[i], big) == 0 {
        // an empty line is a stop too
        if c[i] == '\n' && i + 1 < c.len() && c[i + 1] == '\n' && i + 1 > pos {
            return i + 1;
        }
        i += 1;
    }
    i.min(c.len().saturating_sub(1))
}

fn word_backward(c: &[char], pos: usize, big: bool) -> usize {
    if pos == 0 || c.is_empty() {
        return 0;
    }
    let mut i = pos - 1;
    while i > 0 && class(c[i], big) == 0 {
        i -= 1;
    }
    let k = class(c[i], big);
    while i > 0 && class(c[i - 1], big) == k && k != 0 {
        i -= 1;
    }
    i
}

/// Start of line `n` (0-based, clamped to the last line).
fn line_n(c: &[char], n: usize) -> usize {
    let mut start = 0;
    for _ in 0..n {
        match c[start..].iter().position(|&x| x == '\n') {
            Some(i) => start += i + 1,
            None => break,
        }
    }
    start
}

/// Char indices where `query` starts (ignoring case).
fn matches(text: &str, query: &str) -> Vec<usize> {
    let q = query.to_lowercase();
    let mut out = Vec::new();
    let mut from = 0;
    while from <= text.len() {
        let Some(i) = markdown::find_ci(&text[from..], &q) else { break };
        let at = from + i;
        out.push(markdown::byte_to_char(text, at));
        from = at + text[at..].chars().next().map(|ch| ch.len_utf8()).unwrap_or(1);
    }
    out
}

fn char_range_text(text: &str, a: usize, b: usize) -> String {
    text.chars().skip(a).take(b.saturating_sub(a)).collect()
}

fn replace_chars(text: &mut String, a: usize, b: usize, with: &str) {
    let (ba, bb) = (markdown::char_to_byte(text, a), markdown::char_to_byte(text, b));
    text.replace_range(ba..bb, with);
}

impl Vim {
    /// What the status bar shows.
    pub fn status(&self) -> String {
        if let Some((k, t)) = &self.cmdline {
            return format!("{k}{t}");
        }
        if let Some(m) = &self.message {
            return m.clone();
        }
        let mode = match self.mode {
            Mode::Normal => "NORMAL",
            Mode::Insert => "INSERT",
            Mode::Visual => "VISUAL",
            Mode::VisualLine => "V-LINE",
        };
        if self.pending_g {
            format!("{mode}  g")
        } else {
            mode.to_string()
        }
    }

    pub fn visual(&self) -> bool {
        matches!(self.mode, Mode::Visual | Mode::VisualLine)
    }

    /// The selection as a char range (start, end exclusive) in visual mode.
    pub fn selection(&self, text: &str) -> (usize, usize) {
        let c: Vec<char> = text.chars().collect();
        let (lo, hi) = (self.anchor.min(self.cursor), self.anchor.max(self.cursor));
        match self.mode {
            Mode::VisualLine => (line_start(&c, lo), line_end(&c, hi)),
            _ => (lo, (hi + 1).min(c.len())),
        }
    }

    /// Where visual mode started.
    pub fn anchor(&self) -> usize {
        self.anchor
    }

    /// Put the normal-mode cursor where the text box has it (e.g. after a click), on a character.
    pub fn set_cursor(&mut self, text: &str, cursor: usize) {
        let c: Vec<char> = text.chars().collect();
        self.cursor = clamp_normal(&c, cursor);
    }

    /// Back to normal mode with the cursor on a character (e.g. the note got the keyboard).
    pub fn reset(&mut self, text: &str, cursor: usize) {
        let c: Vec<char> = text.chars().collect();
        *self = Vim { last_search: self.last_search.take(), ..Vim::default() };
        self.cursor = clamp_normal(&c, cursor);
    }

    /// Leave insert mode: the cursor steps back one character, as in vim.
    pub fn leave_insert(&mut self, text: &str, cursor: usize) {
        let c: Vec<char> = text.chars().collect();
        let back = if cursor > line_start(&c, cursor) { cursor - 1 } else { cursor };
        self.mode = Mode::Normal;
        self.want_col = None;
        self.cursor = clamp_normal(&c, back);
    }

    /// Handle one key. `self.cursor` is the cursor before and after.
    pub fn key(&mut self, k: VimKey, text: &mut String, fx: &mut Effects) {
        if self.cmdline.is_some() {
            self.command_key(k, text, fx);
            return;
        }
        self.message = None;
        let c: Vec<char> = text.chars().collect();
        let pos = self.cursor.min(c.len());
        let before = pos;
        let g = std::mem::take(&mut self.pending_g);
        let mut keep_col = false;
        let ch = match k {
            VimKey::Char(ch) => ch,
            VimKey::Left => 'h',
            VimKey::Right => 'l',
            VimKey::Up => 'k',
            VimKey::Down => 'j',
            VimKey::Esc => {
                if self.visual() {
                    self.mode = Mode::Normal;
                    self.cursor = clamp_normal(&c, pos);
                }
                return;
            }
            VimKey::Enter => 'j',
            VimKey::Backspace => 'h',
        };
        let mut new = pos;
        match ch {
            'h' => new = if pos > line_start(&c, pos) { pos - 1 } else { pos },
            'l' => new = if pos < last_on_line(&c, pos) { pos + 1 } else { pos },
            'j' | 'k' => {
                let col = self.want_col.unwrap_or(pos - line_start(&c, pos));
                let target = if ch == 'j' {
                    let e = line_end(&c, pos);
                    (e < c.len()).then_some(e + 1)
                } else {
                    let s = line_start(&c, pos);
                    (s > 0).then(|| line_start(&c, s - 1))
                };
                if let Some(t) = target {
                    new = (t + col).min(last_on_line(&c, t));
                }
                self.want_col = Some(col);
                keep_col = true;
            }
            'w' | 'W' => new = word_forward(&c, pos, ch == 'W'),
            'b' | 'B' => new = word_backward(&c, pos, ch == 'B'),
            '0' => new = line_start(&c, pos),
            '$' => new = last_on_line(&c, pos),
            'g' if g => new = 0,
            'g' => self.pending_g = true,
            'G' => new = line_start(&c, c.len()),
            'n' | 'N' => new = self.search_step(text, pos, ch == 'n'),
            ':' | '/' => self.cmdline = Some((ch, String::new())),
            _ if self.visual() => self.visual_key(ch, text, &c, fx),
            _ => self.normal_key(ch, text, &c, fx),
        }
        if !keep_col {
            self.want_col = None;
        }
        if (self.mode == Mode::Normal || self.visual()) && new != pos {
            self.cursor = if self.visual() { new.min(c.len().saturating_sub(1)) } else { clamp_normal(&c, new) };
        }
        fx.moved |= self.cursor != before;
    }

    /// Keys that change the text or the mode in normal mode.
    fn normal_key(&mut self, ch: char, text: &mut String, c: &[char], fx: &mut Effects) {
        let pos = self.cursor.min(c.len());
        match ch {
            'i' => self.mode = Mode::Insert,
            'a' => {
                self.mode = Mode::Insert;
                self.cursor = (pos + 1).min(line_end(c, pos));
            }
            'I' => {
                self.mode = Mode::Insert;
                self.cursor = first_non_blank(c, pos);
            }
            'A' => {
                self.mode = Mode::Insert;
                self.cursor = line_end(c, pos);
            }
            'o' => {
                let e = line_end(c, pos);
                replace_chars(text, e, e, "\n");
                self.mode = Mode::Insert;
                self.cursor = e + 1;
                fx.changed = true;
            }
            'O' => {
                let s = line_start(c, pos);
                replace_chars(text, s, s, "\n");
                self.mode = Mode::Insert;
                self.cursor = s;
                fx.changed = true;
            }
            'x' if pos < line_end(c, pos) => {
                replace_chars(text, pos, pos + 1, "");
                let c2: Vec<char> = text.chars().collect();
                self.cursor = clamp_normal(&c2, pos);
                fx.changed = true;
            }
            'v' | 'V' => {
                self.mode = if ch == 'v' { Mode::Visual } else { Mode::VisualLine };
                self.anchor = pos;
            }
            'p' => {
                self.paste_pending = true;
                fx.request_paste = true;
            }
            _ => {}
        }
    }

    /// d / y / v / V / o in visual mode.
    fn visual_key(&mut self, ch: char, text: &mut String, c: &[char], fx: &mut Effects) {
        match ch {
            'v' | 'V' => {
                let want = if ch == 'v' { Mode::Visual } else { Mode::VisualLine };
                self.mode = if self.mode == want { Mode::Normal } else { want };
                if self.mode == Mode::Normal {
                    self.cursor = clamp_normal(c, self.cursor);
                }
            }
            'd' | 'y' | 'x' => {
                let line = self.mode == Mode::VisualLine;
                let (a, b) = self.selection(text);
                // whole lines take their line break with them
                let (mut da, mut db) = (a, b);
                if line {
                    if db < c.len() {
                        db += 1;
                    } else {
                        da = da.saturating_sub(1);
                    }
                }
                let mut taken = char_range_text(text, a, b);
                if line {
                    taken.push('\n');
                }
                fx.copy = Some(taken);
                if ch == 'y' {
                    self.cursor = if line { line_start(c, a) } else { a };
                } else {
                    replace_chars(text, da, db, "");
                    fx.changed = true;
                    let c2: Vec<char> = text.chars().collect();
                    self.cursor = clamp_normal(&c2, if line { line_start(&c2, da.min(c2.len())) } else { a });
                    if line && da < a {
                        // the last lines went: stand on the line before them
                        self.cursor = clamp_normal(&c2, line_start(&c2, da));
                    }
                }
                self.mode = Mode::Normal;
                let c3: Vec<char> = text.chars().collect();
                self.cursor = clamp_normal(&c3, self.cursor);
            }
            _ => {}
        }
    }

    fn search_step(&mut self, text: &str, pos: usize, forward: bool) -> usize {
        let Some(q) = self.last_search.clone().filter(|q| !q.is_empty()) else {
            self.message = Some("No previous search".into());
            return pos;
        };
        let all = matches(text, &q);
        if all.is_empty() {
            self.message = Some(format!("Pattern not found: {q}"));
            return pos;
        }
        let hit = if forward { all.iter().copied().find(|&m| m > pos) } else { all.iter().rev().copied().find(|&m| m < pos) };
        match hit {
            Some(m) => m,
            None => {
                self.message = Some(if forward { "search hit BOTTOM, continuing at TOP".into() } else { "search hit TOP, continuing at BOTTOM".into() });
                if forward {
                    all[0]
                } else {
                    *all.last().unwrap()
                }
            }
        }
    }

    /// Keys while `:` or `/` is being typed.
    fn command_key(&mut self, k: VimKey, text: &mut str, fx: &mut Effects) {
        let Some((kind, mut line)) = self.cmdline.take() else { return };
        match k {
            VimKey::Esc => {}
            VimKey::Backspace => {
                if line.pop().is_some() {
                    self.cmdline = Some((kind, line));
                }
            }
            VimKey::Char(ch) => {
                line.push(ch);
                self.cmdline = Some((kind, line));
            }
            VimKey::Enter => {
                let c: Vec<char> = text.chars().collect();
                let before = self.cursor;
                if kind == ':' {
                    match line.trim().parse::<usize>() {
                        Ok(n) => self.cursor = clamp_normal(&c, line_n(&c, n.saturating_sub(1))),
                        Err(_) if line.trim().is_empty() => {}
                        Err(_) => self.message = Some(format!("Not a line number: {line}")),
                    }
                } else {
                    self.last_search = Some(line);
                    let to = self.search_step(text, self.cursor, true);
                    self.cursor = if self.visual() { to } else { clamp_normal(&c, to) };
                }
                fx.moved |= self.cursor != before;
            }
            _ => self.cmdline = Some((kind, line)),
        }
    }

    /// The clipboard text arrived after `p`: put it after the cursor, or below the line when it
    /// is whole lines.
    pub fn paste(&mut self, clip: &str, text: &mut String, fx: &mut Effects) {
        self.paste_pending = false;
        if clip.is_empty() {
            return;
        }
        let c: Vec<char> = text.chars().collect();
        let pos = self.cursor.min(c.len());
        if clip.ends_with('\n') {
            let e = line_end(&c, pos);
            if e < c.len() {
                replace_chars(text, e + 1, e + 1, clip);
                self.cursor = e + 1;
            } else {
                let body = clip.trim_end_matches('\n');
                replace_chars(text, e, e, &format!("\n{body}"));
                self.cursor = e + 1;
            }
        } else {
            let at = if line_end(&c, pos) > line_start(&c, pos) { pos + 1 } else { pos };
            replace_chars(text, at, at, clip);
            self.cursor = at + clip.chars().count() - 1;
        }
        let c2: Vec<char> = text.chars().collect();
        self.cursor = clamp_normal(&c2, self.cursor);
        fx.changed = true;
        fx.moved = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run keys (typed characters; `<esc>`, `<cr>`, `<bs>` for the others) from `cursor`.
    fn run(text: &str, cursor: usize, keys: &str) -> (Vim, String, Effects) {
        let mut v = Vim::default();
        let mut t = text.to_string();
        v.reset(&t, cursor);
        let mut fx = Effects::default();
        let mut rest = keys;
        while !rest.is_empty() {
            let (k, n) = if let Some(r) = rest.strip_prefix("<esc>") {
                (VimKey::Esc, rest.len() - r.len())
            } else if let Some(r) = rest.strip_prefix("<cr>") {
                (VimKey::Enter, rest.len() - r.len())
            } else if let Some(r) = rest.strip_prefix("<bs>") {
                (VimKey::Backspace, rest.len() - r.len())
            } else {
                let ch = rest.chars().next().unwrap();
                (VimKey::Char(ch), ch.len_utf8())
            };
            if v.mode == Mode::Insert {
                // in insert mode the text box types; only Esc comes back here
                if k == VimKey::Esc {
                    let c = v.cursor;
                    v.leave_insert(&t, c);
                } else if let VimKey::Char(ch) = k {
                    replace_chars(&mut t, v.cursor, v.cursor, &ch.to_string());
                    v.cursor += 1;
                }
            } else {
                v.key(k, &mut t, &mut fx);
            }
            rest = &rest[n..];
        }
        (v, t, fx)
    }

    fn at(text: &str, cursor: usize, keys: &str) -> usize {
        run(text, cursor, keys).0.cursor
    }

    const T: &str = "one two.three  four\n\n  indented line\nlast";

    // spec: EDIT-011
    #[test]
    fn motions_move_like_vim() {
        assert_eq!(at(T, 0, "l"), 1);
        assert_eq!(at(T, 0, "h"), 0, "h stops at the line start");
        assert_eq!(at(T, 0, "$"), 18, "$ goes to the last character, not the break");
        assert_eq!(at(T, 18, "l"), 18, "l stops at the line end");
        assert_eq!(at(T, 5, "0"), 0);
        // words: punctuation is its own word, W only splits at blanks
        assert_eq!(at(T, 0, "w"), 4);
        assert_eq!(at(T, 4, "w"), 7, "two|.");
        assert_eq!(at(T, 7, "w"), 8, ".|three");
        assert_eq!(at(T, 4, "W"), 15, "two.three is one WORD");
        assert_eq!(at(T, 15, "w"), 20, "the empty line is a stop");
        assert_eq!(at(T, 15, "b"), 8);
        assert_eq!(at(T, 15, "B"), 4);
        assert_eq!(at(T, 0, "b"), 0);
        // j / k keep the column, also across the empty line
        assert_eq!(at(T, 5, "j"), 20, "empty line");
        assert_eq!(at(T, 5, "jj"), 26, "column 5 again");
        assert_eq!(at(T, 5, "jjj"), 40, "last line is shorter: its last character");
        assert_eq!(at(T, 5, "jjjk"), 26);
        assert_eq!(at(T, 0, "G"), 37);
        assert_eq!(at(T, 30, "gg"), 0);
        assert_eq!(at("é ü ß", 0, "ll$"), 4, "characters, not bytes");
    }

    // spec: EDIT-012
    #[test]
    fn colon_line_numbers_and_slash_search() {
        assert_eq!(at(T, 0, ":3<cr>"), 21);
        assert_eq!(at(T, 0, ":99<cr>"), 37, "clamped to the last line");
        assert_eq!(at(T, 0, ":4<bs>3<cr>"), 21, "backspace edits the command");
        assert_eq!(at(T, 0, ":3<esc>"), 0, "Esc cancels");
        let (v, _, _) = run(T, 0, ":3");
        assert_eq!(v.status(), ":3");
        // search ignores case, n / N go on and wrap around
        let text = "Apple pie\napple tree\nno APPLE";
        assert_eq!(at(text, 0, "/apple<cr>"), 10);
        assert_eq!(at(text, 0, "/apple<cr>n"), 24);
        let (v, _, _) = run(text, 0, "/apple<cr>nn");
        assert_eq!((v.cursor, v.message.as_deref()), (0, Some("search hit BOTTOM, continuing at TOP")));
        assert_eq!(at(text, 0, "/apple<cr>N"), 0);
        let (v, _, _) = run(text, 0, "/pear<cr>");
        assert_eq!((v.cursor, v.message.as_deref()), (0, Some("Pattern not found: pear")));
        assert_eq!(at("Straße STRAẞE", 0, "/straße<cr>"), 7);
    }

    // spec: EDIT-013
    #[test]
    fn entering_and_leaving_insert_mode() {
        assert_eq!(run("abc", 1, "iX<esc>").1, "aXbc");
        assert_eq!(run("abc", 1, "aX<esc>").1, "abXc");
        assert_eq!(run("  abc", 4, "IX<esc>").1, "  Xabc");
        assert_eq!(run("abc\nd", 0, "AX<esc>").1, "abcX\nd");
        assert_eq!(run("abc\nd", 0, "oX<esc>").1, "abc\nX\nd");
        assert_eq!(run("abc\nd", 4, "OX<esc>").1, "abc\nX\nd");
        let (v, _, _) = run("abc", 0, "AX<esc>");
        assert_eq!((v.mode, v.cursor), (Mode::Normal, 3), "Esc steps back onto the typed X");
        assert_eq!(run("abc", 1, "x").1, "ac");
        assert_eq!(run("abc", 2, "x").0.cursor, 1, "x at the line end keeps the cursor on the line");
        assert_eq!(run("\nabc", 0, "x").1, "\nabc", "x on an empty line does nothing");
        assert_eq!(run("abc", 0, "jkd").1, "abc", "keys without meaning change nothing");
    }

    // spec: EDIT-014
    #[test]
    fn visual_mode_deletes_and_copies() {
        // v + motion + d: the selection goes to the clipboard
        let (v, t, fx) = run("one two three", 4, "vwd");
        assert_eq!(t, "one hree");
        assert_eq!(fx.copy.as_deref(), Some("two t"));
        assert_eq!((v.mode, v.cursor), (Mode::Normal, 4));
        let (_, t, fx) = run("one two three", 8, "v$y");
        assert_eq!((t.as_str(), fx.copy.as_deref()), ("one two three", Some("three")), "y copies and changes nothing");
        // backwards selection
        let (_, t, fx) = run("one two three", 8, "vbd");
        assert_eq!((t.as_str(), fx.copy.as_deref()), ("one hree", Some("two t")));
        // V: whole lines, with their line break
        let (v, t, fx) = run("a\nb\nc\nd", 2, "Vjd");
        assert_eq!((t.as_str(), fx.copy.as_deref()), ("a\nd", Some("b\nc\n")));
        assert_eq!(v.cursor, 2);
        let (_, t, fx) = run("a\nb\nc", 4, "Vd");
        assert_eq!((t.as_str(), fx.copy.as_deref()), ("a\nb", Some("c\n")), "the last line takes the break before it");
        let (v, _, _) = run("a\nb", 0, "vj");
        assert_eq!((v.mode, v.selection("a\nb")), (Mode::Visual, (0, 3)));
        let (v, _, _) = run("a\nb", 0, "v<esc>");
        assert_eq!(v.mode, Mode::Normal);
        assert_eq!(run("a\nb", 0, "Vv").0.mode, Mode::Visual, "V then v switches to character-wise");
    }

    // spec: EDIT-014
    #[test]
    fn p_pastes_the_clipboard() {
        let mut v = Vim::default();
        let mut t = "abc\ndef".to_string();
        v.reset(&t, 1);
        let mut fx = Effects::default();
        v.key(VimKey::Char('p'), &mut t, &mut fx);
        assert!(fx.request_paste && v.paste_pending, "p asks the window for the clipboard");
        v.paste("XY", &mut t, &mut fx);
        assert_eq!((t.as_str(), v.cursor), ("abXYc\ndef", 3), "after the cursor, on the last pasted character");
        assert!(!v.paste_pending);
        // whole lines go below the current line
        v.paste("new line\n", &mut t, &mut fx);
        assert_eq!(t, "abXYc\nnew line\ndef");
        assert_eq!(v.cursor, 6);
        let mut t = "last".to_string();
        v.reset(&t, 0);
        v.paste("below\n", &mut t, &mut fx);
        assert_eq!(t, "last\nbelow");
        let mut t = String::new();
        v.reset(&t, 0);
        v.paste("x", &mut t, &mut fx);
        assert_eq!(t, "x");
    }

    // spec: EDIT-010
    #[test]
    fn status_shows_the_mode() {
        assert_eq!(run("a", 0, "").0.status(), "NORMAL");
        assert_eq!(run("a", 0, "v").0.status(), "VISUAL");
        assert_eq!(run("a", 0, "V").0.status(), "V-LINE");
        assert_eq!(run("a", 0, "i").0.status(), "INSERT");
        assert_eq!(run("a", 0, "/ab").0.status(), "/ab");
    }
}
