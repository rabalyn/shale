//! The markdown editor: a text box that colours markdown as you type, shows links in
//! blue (note exists) or red (note missing), follows links on click and suggests note
//! names after `[[`.

#[cfg(test)]
use crate::link_assist::link_query;
use crate::markdown::{self, LinkSpan};
use crate::theme;
use crate::vault::Vault;
use eframe::egui::{self, text::LayoutJob, Color32, FontFamily, FontId, Galley, Key, Modifiers, Pos2, Rect, Stroke, TextFormat, Ui, Vec2};
use egui::text::{CCursor, CCursorRange};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::path::Path;
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq, Default, Hash)]
struct Fmt {
    heading: u8,
    bold: bool,
    italic: bool,
    code: bool,
    fence: bool,
    /// 0 none, 1 existing link, 2 missing link, 3 web link
    link: u8,
    dim: bool,
    tag: bool,
    strike: bool,
    quote: bool,
    marker: bool,
    highlight: bool,
    /// > 0: this text is an image link shown as a picture; the row is this many points tall
    image_h: u16,
}

pub fn heading_scale(level: u8) -> f32 {
    match level {
        1 => 1.75,
        2 => 1.5,
        3 => 1.3,
        4 => 1.15,
        5 | 6 => 1.05,
        _ => 1.0,
    }
}

fn set(fmts: &mut [Fmt], r: Range<usize>, f: impl Fn(&mut Fmt)) {
    let end = r.end.min(fmts.len());
    for x in &mut fmts[r.start.min(end)..end] {
        f(x);
    }
}

/// Toggle-style inline markers inside one line (`**bold**`, `*it*`, `~~s~~`, `==h==`).
fn inline_pairs(line: &str, off: usize, fmts: &mut [Fmt], code: &[Range<usize>]) {
    let b = line.as_bytes();
    let in_code = |i: usize| code.iter().any(|r| r.contains(&(off + i)));
    for (marker, kind) in [("**", 0u8), ("__", 0), ("~~", 1), ("==", 2)] {
        let mut i = 0;
        while let Some(p) = line[i..].find(marker) {
            let s = i + p;
            if in_code(s) {
                i = s + 2;
                continue;
            }
            if let Some(q) = line[s + 2..].find(marker) {
                let e = s + 2 + q;
                if q > 0 && !in_code(e) {
                    set(fmts, off + s..off + s + 2, |f| f.dim = true);
                    set(fmts, off + e..off + e + 2, |f| f.dim = true);
                    set(fmts, off + s + 2..off + e, |f| match kind {
                        0 => f.bold = true,
                        1 => f.strike = true,
                        _ => f.highlight = true,
                    });
                    i = e + 2;
                    continue;
                }
            }
            break;
        }
    }
    // single * or _ italics (not part of ** and not list markers)
    for m in *b"*_" {
        let mut i = 0;
        while i < b.len() {
            let is_single = |j: usize| b[j] == m && (j == 0 || b[j - 1] != m) && (j + 1 >= b.len() || b[j + 1] != m);
            if is_single(i) && !in_code(i) && i + 1 < b.len() && b[i + 1] != b' ' {
                if m == b'_' && i > 0 && b[i - 1].is_ascii_alphanumeric() {
                    i += 1;
                    continue;
                }
                let mut j = i + 1;
                let mut found = None;
                while j < b.len() {
                    if is_single(j) && b[j - 1] != b' ' {
                        found = Some(j);
                        break;
                    }
                    j += 1;
                }
                if let Some(j) = found {
                    set(fmts, off + i..off + i + 1, |f| f.dim = true);
                    set(fmts, off + j..off + j + 1, |f| f.dim = true);
                    set(fmts, off + i + 1..off + j, |f| f.italic = true);
                    i = j + 1;
                    continue;
                }
            }
            i += 1;
        }
    }
}

/// Build the coloured layout for the editor (without inline pictures).
#[cfg(test)]
pub fn highlight(text: &str, links: &[(LinkSpan, bool)], base: f32) -> LayoutJob {
    highlight_with_images(text, links, base, &[])
}

/// Like [`highlight`], but the given byte ranges (image links) are shrunk to invisible text on a
/// row tall enough for the picture that is painted there afterwards.
pub fn highlight_with_images(text: &str, links: &[(LinkSpan, bool)], base: f32, images: &[(Range<usize>, f32)]) -> LayoutJob {
    let mut fmts = vec![Fmt::default(); text.len()];
    let fenced = markdown::fenced_code_lines(text);
    let mut code_ranges: Vec<Range<usize>> = Vec::new();
    let mut in_front = false;
    for (li, (off, line)) in markdown::lines_with_offsets(text).into_iter().enumerate() {
        let lr = off..off + line.len();
        // YAML front matter at the top
        if li == 0 && line.trim() == "---" {
            in_front = true;
            set(&mut fmts, lr, |f| f.dim = true);
            continue;
        }
        if in_front {
            set(&mut fmts, lr.clone(), |f| {
                f.code = true;
                f.dim = true
            });
            if line.trim() == "---" {
                in_front = false;
            }
            continue;
        }
        if fenced.get(li).copied().unwrap_or(false) {
            set(&mut fmts, lr, |f| f.fence = true);
            continue;
        }
        if let Some(level) = markdown::heading_level(line) {
            set(&mut fmts, lr.clone(), |f| f.heading = level as u8);
            set(&mut fmts, off..off + level, |f| f.dim = true);
        }
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        if trimmed.starts_with('>') {
            set(&mut fmts, lr.clone(), |f| f.quote = true);
            set(&mut fmts, off + indent..off + indent + 1, |f| f.dim = true);
        }
        if trimmed == "---" || trimmed == "***" || trimmed == "___" {
            set(&mut fmts, lr.clone(), |f| f.dim = true);
        }
        if let Some((check, done, _, _)) = markdown::parse_task_line(line) {
            set(&mut fmts, off + indent..off + check + 2, |f| f.marker = true);
            if done {
                set(&mut fmts, off + check + 2..lr.end, |f| f.strike = true);
            }
        } else if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("+ ") {
            set(&mut fmts, off + indent..off + indent + 1, |f| f.marker = true);
        } else {
            let digits = trimmed.bytes().take_while(|c| c.is_ascii_digit()).count();
            if digits > 0 && trimmed[digits..].starts_with(". ") {
                set(&mut fmts, off + indent..off + indent + digits + 1, |f| f.marker = true);
            }
        }
        // inline code
        let lb = line.as_bytes();
        let mut i = 0;
        let mut local_code = Vec::new();
        while i < lb.len() {
            if lb[i] == b'`' {
                if let Some(e) = line[i + 1..].find('`') {
                    let r = off + i..off + i + e + 2;
                    set(&mut fmts, r.clone(), |f| f.code = true);
                    local_code.push(r);
                    i += e + 2;
                    continue;
                }
            }
            i += 1;
        }
        inline_pairs(line, off, &mut fmts, &local_code);
        code_ranges.extend(local_code);
    }
    for (l, exists) in links {
        set(&mut fmts, l.range.clone(), |f| {
            f.dim = true;
            f.italic = false;
        });
        let kind = if *exists { 1 } else { 2 };
        // colour the shown part (target, or alias) strongly
        let shown = if l.markdown_style {
            // [label](target): colour the label
            let start = l.range.start + if l.embed { 2 } else { 1 };
            let label_end = text[start..l.range.end].find("](").map(|p| start + p).unwrap_or(l.range.end);
            start..label_end
        } else {
            let inner_start = l.range.start + if l.embed { 3 } else { 2 };
            let inner_end = l.range.end - 2;
            let inner = &text[inner_start..inner_end];
            match inner.find('|') {
                Some(p) => inner_start + p + 1..inner_end,
                None => inner_start..inner_end,
            }
        };
        set(&mut fmts, l.range.clone(), |f| f.link = kind);
        set(&mut fmts, shown, |f| f.dim = false);
    }
    for w in crate::lists::find_web_links(text) {
        if code_ranges.iter().any(|c| c.contains(&w.range.start)) {
            continue;
        }
        set(&mut fmts, w.range.clone(), |f| {
            f.dim = true;
            f.link = 3;
        });
        set(&mut fmts, w.label_range.clone(), |f| f.dim = false);
    }
    for (r, _) in markdown::find_tags(text) {
        if !code_ranges.iter().any(|c| c.contains(&r.start)) {
            set(&mut fmts, r, |f| f.tag = true);
        }
    }

    for (r, h) in images {
        if r.end <= text.len() && text.is_char_boundary(r.start) && text.is_char_boundary(r.end) {
            set(&mut fmts, r.clone(), |f| f.image_h = h.round().clamp(1.0, 4000.0) as u16);
        }
    }
    let mut job = LayoutJob { text: text.to_string(), ..Default::default() };
    let mut start = 0;
    let mut cur: Option<Fmt> = None;
    for (i, _) in text.char_indices().chain(std::iter::once((text.len(), ' '))) {
        let f = if i < text.len() { Some(fmts[i]) } else { None };
        if f != cur || i == text.len() {
            if let Some(c) = cur {
                if i > start {
                    job.sections.push(egui::text::LayoutSection { leading_space: 0.0, byte_range: egui::text::ByteIndex(start)..egui::text::ByteIndex(i), format: to_format(c, base) });
                }
            }
            start = i;
            cur = f;
        }
    }
    if job.sections.is_empty() {
        job.sections.push(egui::text::LayoutSection { leading_space: 0.0, byte_range: egui::text::ByteIndex(0)..egui::text::ByteIndex(text.len()), format: to_format(Fmt::default(), base) });
    }
    job
}

fn to_format(f: Fmt, base: f32) -> TextFormat {
    if f.image_h > 0 {
        return TextFormat { font_id: FontId::new(1.0, FontFamily::Proportional), color: Color32::TRANSPARENT, line_height: Some(f.image_h as f32), ..Default::default() };
    }
    let size = base * heading_scale(f.heading);
    let mono = f.code || f.fence;
    let font_id = if mono { FontId::new(base * 0.92 * heading_scale(f.heading), FontFamily::Monospace) } else { FontId::new(size, FontFamily::Proportional) };
    let mut color = if f.heading > 0 { theme::HEADING } else { theme::TEXT };
    if f.quote {
        color = theme::QUOTE;
    }
    if f.bold {
        color = theme::TEXT_STRONG;
    }
    if mono {
        color = theme::CODE;
    }
    if f.marker {
        color = theme::ACCENT;
    }
    if f.tag {
        color = theme::TAG;
    }
    if f.strike {
        color = theme::DONE;
    }
    match f.link {
        1 | 3 => color = theme::LINK,
        2 => color = theme::LINK_MISSING,
        _ => {}
    }
    if f.dim {
        color = if f.link == 1 || f.link == 3 {
            theme::LINK.gamma_multiply(0.45)
        } else if f.link == 2 {
            theme::LINK_MISSING.gamma_multiply(0.45)
        } else {
            theme::FAINT
        };
    }
    let mut tf = TextFormat { font_id, color, italics: f.italic || f.quote, line_height: Some(size * 1.5), ..Default::default() };
    if f.fence || (f.code && !f.dim) {
        tf.background = theme::CODE_BG;
    }
    if f.highlight {
        tf.background = Color32::from_rgba_unmultiplied(200, 170, 40, 60);
    }
    if f.tag {
        tf.background = theme::TAG_BG;
    }
    if f.link > 0 && !f.dim {
        tf.underline = Stroke::new(1.0, color.gamma_multiply(0.5));
    }
    if f.strike && !f.dim {
        tf.strikethrough = Stroke::new(1.0, theme::DONE);
    }
    if f.bold {
        tf.extra_letter_spacing = 0.3;
    }
    tf
}

/// Something shown in place of an embed link in the editor.
enum Art {
    Tex(egui::TextureHandle),
    /// a drawing and its path (its pictures are found from there)
    Scene(Arc<crate::drawing::Scene>, std::path::PathBuf),
    /// first page of a PDF, and its page count
    Pdf(egui::TextureHandle, usize),
}

/// What the editor asks the app to do.
pub enum EditorAction {
    OpenLink {
        target: String,
        heading: Option<String>,
        new_tab: bool,
    },
    /// A web link: open it in the default browser.
    OpenUrl(String),
    /// A file was copied into the vault; the file list should be refreshed.
    FilesChanged,
    /// `[[p Name` picked "new contact": make a contact note in the people folder
    CreatePerson(String),
}

#[derive(Default)]
struct Cache {
    key: u64,
    links: Vec<(LinkSpan, bool)>,
}

pub struct NoteEditor {
    pub id: egui::Id,
    cache: Cache,
    /// `[[`, `[[p ` and `[!` helpers of the text box
    pub assist: crate::link_assist::LinkAssist,
    /// Put the cursor here (char index) on the next frame and scroll to it.
    pub pending_cursor: Option<usize>,
    pub request_focus: bool,
    /// Where the text was drawn last frame (used by tests and for scrolling).
    pub last_layout: Option<(Arc<Galley>, Pos2)>,
    /// Draw embedded images, drawings and PDFs inside the editor (set from the app's settings).
    pub inline_images: bool,
    /// Draw embedded drawings on a dark canvas (set from the app's settings).
    pub dark_canvas: bool,
    last_cursor: Option<(usize, usize)>,
    /// vim-lite editing is on (set from the app's settings)
    pub vim_enabled: bool,
    pub vim: crate::vim::Vim,
    /// the editor had the keyboard last frame / when it lost it (seconds)
    vim_was_focused: bool,
    vim_lost_at: Option<f64>,
    /// the cursor range vim put into the text box last (a different one means a mouse click)
    vim_expected: Option<(usize, usize)>,
    /// scroll this char into view after the text box was drawn
    vim_scroll: Option<usize>,
}

impl NoteEditor {
    pub fn new(id: egui::Id) -> Self {
        NoteEditor {
            id,
            cache: Cache::default(),
            assist: crate::link_assist::LinkAssist::new(id),
            pending_cursor: None,
            request_focus: false,
            last_layout: None,
            inline_images: false,
            dark_canvas: true,
            last_cursor: None,
            vim_enabled: false,
            vim: crate::vim::Vim::default(),
            vim_was_focused: false,
            vim_lost_at: None,
            vim_expected: None,
            vim_scroll: None,
        }
    }

    /// Is the `[!` file picker popup on screen?
    pub fn picker_visible(&self) -> bool {
        self.assist.picker_visible()
    }

    /// Short description of the picker state (testing aid).
    pub fn debug_state(&self) -> String {
        self.assist.debug_state()
    }

    /// Typing goes into the text (vim-lite off, or in insert mode).
    fn typing(&self) -> bool {
        !self.vim_enabled || self.vim.mode == crate::vim::Mode::Insert
    }

    /// This editor has the keyboard in vim normal mode, not typing a `:` or `/` command: the
    /// keys of the "Editor normal mode" scope (EDIT-015) are looked up before vim sees them.
    pub fn vim_normal_keys(&self, ctx: &egui::Context) -> bool {
        self.vim_enabled && self.vim.mode == crate::vim::Mode::Normal && !self.vim.typing_command() && ctx.memory(|m| m.has_focus(self.id))
    }

    /// The vim mode (or command) for the status bar, while this editor has the keyboard.
    pub fn vim_status(&self) -> Option<String> {
        (self.vim_enabled && self.vim_was_focused).then(|| self.vim.status())
    }

    fn text_cursor(&self, ui: &Ui) -> Option<(usize, usize)> {
        egui::text_edit::TextEditState::load(ui.ctx(), self.id).and_then(|st| st.cursor.char_range()).map(|r| (r.primary.index.0, r.secondary.index.0))
    }

    /// Put vim's cursor or selection into the text box.
    fn vim_apply_cursor(&mut self, ui: &Ui, text: &str) {
        let range = if self.vim.visual() {
            let (a, b) = self.vim.selection(text);
            // the cursor is drawn at the moving end
            if self.vim.cursor >= self.vim.anchor() {
                CCursorRange::two(CCursor::new(a), CCursor::new(b))
            } else {
                CCursorRange::two(CCursor::new(b), CCursor::new(a))
            }
        } else {
            CCursorRange::one(CCursor::new(self.vim.cursor))
        };
        let mut state = egui::text_edit::TextEditState::load(ui.ctx(), self.id).unwrap_or_default();
        state.cursor.set_char_range(Some(range));
        state.store(ui.ctx(), self.id);
        self.vim_expected = Some((range.primary.index.0, range.secondary.index.0));
    }

    /// Vim-lite for this frame: outside insert mode every key goes to vim instead of the text.
    /// Returns true when the text changed.
    fn vim_frame(&mut self, ui: &mut Ui, text: &mut String) -> bool {
        use crate::vim::{Effects, Mode, VimKey};
        let focused = ui.ctx().memory(|m| m.has_focus(self.id));
        let now = ui.input(|i| i.time);
        if focused && !self.vim_was_focused {
            // the note got the keyboard: normal mode (a blink of lost focus keeps insert mode)
            let blink = self.vim.mode == Mode::Insert && self.vim_lost_at.is_some_and(|t| now - t < 0.5);
            if !blink {
                let cur = self.text_cursor(ui).map(|c| c.0).unwrap_or(0);
                self.vim.reset(text, cur);
                self.vim_apply_cursor(ui, text);
            }
        }
        if !focused && self.vim_was_focused {
            self.vim_lost_at = Some(now);
        }
        self.vim_was_focused = focused;
        if !focused {
            return false;
        }
        if self.vim.mode == Mode::Insert {
            // Esc back to normal mode (an open popup took its Esc already, above)
            if !self.assist.is_open() && !self.assist.picker_visible() && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
                let cur = self.text_cursor(ui).map(|c| c.0).unwrap_or(0);
                self.vim.leave_insert(text, cur);
                self.vim_apply_cursor(ui, text);
            }
            return false;
        }
        // where the text box has the cursor: a click there moves vim's cursor (and ends visual)
        if let Some(tc) = self.text_cursor(ui) {
            if Some(tc) != self.vim_expected {
                if self.vim.visual() {
                    self.vim.reset(text, tc.0);
                } else {
                    self.vim.set_cursor(text, tc.0);
                }
                self.vim_apply_cursor(ui, text);
            }
        }
        // take every key that would type or move in the text box; Ctrl/Alt chords stay
        let paste_pending = self.vim.paste_pending;
        let (keys, pastes) = ui.input_mut(|i| {
            let (mut keys, mut pastes) = (Vec::new(), Vec::new());
            i.events.retain(|e| match e {
                egui::Event::Text(t) => {
                    keys.extend(t.chars().map(VimKey::Char));
                    false
                }
                egui::Event::Key { key, pressed, modifiers, .. } if !(modifiers.ctrl || modifiers.command || modifiers.alt || modifiers.mac_cmd) => {
                    if *pressed {
                        let k = match key {
                            Key::Escape => Some(VimKey::Esc),
                            Key::Enter => Some(VimKey::Enter),
                            Key::Backspace => Some(VimKey::Backspace),
                            Key::ArrowLeft => Some(VimKey::Left),
                            Key::ArrowRight => Some(VimKey::Right),
                            Key::ArrowUp => Some(VimKey::Up),
                            Key::ArrowDown => Some(VimKey::Down),
                            _ => None,
                        };
                        keys.extend(k);
                    }
                    false
                }
                egui::Event::Paste(p) if paste_pending => {
                    pastes.push(p.clone());
                    false
                }
                egui::Event::Ime(_) => false,
                _ => true,
            });
            (keys, pastes)
        });
        let mut fx = Effects::default();
        for p in pastes {
            self.vim.paste(&p, text, &mut fx);
        }
        let mut typed_after = String::new();
        for k in keys {
            if self.vim.mode == Mode::Insert {
                // keys after `i` in the same frame are typed (scripted input sends them together)
                if let VimKey::Char(ch) = k {
                    typed_after.push(ch);
                }
                continue;
            }
            self.vim.key(k, text, &mut fx);
        }
        if let Some(c) = fx.copy.take() {
            ui.ctx().copy_text(c);
        }
        if fx.request_paste {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::RequestPaste);
        }
        self.vim_apply_cursor(ui, text);
        if !typed_after.is_empty() {
            ui.input_mut(|i| i.events.push(egui::Event::Text(typed_after)));
        }
        if fx.moved || fx.changed {
            self.vim_scroll = Some(self.vim.cursor);
        }
        if self.vim.mode != Mode::Insert {
            // no input method outside insert mode: keys must arrive as they are
            ui.ctx().output_mut(|o| o.ime = None);
        }
        fx.changed
    }

    fn refresh_links(&mut self, text: &str, vault: &Vault, rel: &Path) {
        let mut h = DefaultHasher::new();
        text.hash(&mut h);
        vault.generation.hash(&mut h);
        rel.hash(&mut h);
        let key = h.finish();
        if key != self.cache.key {
            self.cache.key = key;
            self.cache.links = markdown::find_links(text)
                .into_iter()
                .map(|l| {
                    let ok = vault.exists(&l.target, Some(rel));
                    (l, ok)
                })
                .collect();
        }
    }

    /// Show the editor. Returns (text changed, action).
    pub fn show(&mut self, ui: &mut Ui, text: &mut String, vault: &Vault, rel: &Path, base_size: f32, ctrl_to_follow: bool) -> (bool, Option<EditorAction>) {
        let mut action = None;
        let mut changed = false;
        self.refresh_links(text, vault, rel);

        // Keys for the `[[` list and the `[!` picker must be taken before the text box sees them.
        self.assist.id = self.id;
        let mut keys = self.assist.take_keys(ui, text, vault);
        changed |= keys.changed;
        if keys.action.is_some() {
            action = keys.action.take();
        }
        let media_handled = keys.handled;

        if self.vim_enabled && self.vim_frame(ui, text) {
            changed = true;
            self.refresh_links(text, vault, rel);
        }

        // Smart lists and headings: Enter continues a list, Tab / Shift+Tab indent or change levels,
        // Alt+Up / Alt+Down move lines.
        if !media_handled && !keys.picker_open && !keys.picking && ui.ctx().memory(|m| m.has_focus(self.id)) {
            if let Some(range) = egui::text_edit::TextEditState::load(ui.ctx(), self.id).and_then(|st| st.cursor.char_range()) {
                let a = markdown::char_to_byte(text, range.primary.index.0);
                let b = markdown::char_to_byte(text, range.secondary.index.0);
                let sel = a.min(b)..a.max(b);
                let (alt_up, alt_down) = ui.input_mut(|i| (i.consume_key(Modifiers::ALT, Key::ArrowUp), i.consume_key(Modifiers::ALT, Key::ArrowDown)));
                let (enter, tab, shift_tab) = ui.input(|i| {
                    (
                        i.events.iter().any(|e| matches!(e, egui::Event::Key { key: Key::Enter, pressed: true, modifiers, .. } if modifiers.is_none())),
                        i.events.iter().any(|e| matches!(e, egui::Event::Key { key: Key::Tab, pressed: true, modifiers, .. } if modifiers.is_none())),
                        i.events.iter().any(|e| matches!(e, egui::Event::Key { key: Key::Tab, pressed: true, modifiers, .. } if modifiers.shift_only())),
                    )
                });
                let mut result: Option<(String, std::ops::Range<usize>)> = None;
                if alt_up || alt_down {
                    result = crate::lists::move_lines(text, sel.clone(), alt_up);
                } else if enter && sel.is_empty() {
                    if let Some((t, c)) = crate::lists::continue_list(text, sel.start) {
                        ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));
                        result = Some((t, c..c));
                    }
                } else if tab || shift_tab {
                    if let Some(r) = crate::lists::indent(text, sel.clone(), shift_tab) {
                        ui.input_mut(|i| {
                            i.consume_key(Modifiers::NONE, Key::Tab);
                            i.consume_key(Modifiers::SHIFT, Key::Tab);
                        });
                        result = Some(r);
                    }
                }
                if let Some((t, r)) = result {
                    *text = t;
                    changed = true;
                    let cs = markdown::byte_to_char(text, r.start);
                    let ce = markdown::byte_to_char(text, r.end);
                    let mut state = egui::text_edit::TextEditState::load(ui.ctx(), self.id).unwrap_or_default();
                    state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(cs), CCursor::new(ce))));
                    state.store(ui.ctx(), self.id);
                    self.refresh_links(text, vault, rel);
                }
            }
        }

        if let Some(c) = self.pending_cursor {
            let mut state = egui::text_edit::TextEditState::load(ui.ctx(), self.id).unwrap_or_default();
            state.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(c))));
            state.store(ui.ctx(), self.id);
            ui.ctx().memory_mut(|m| m.request_focus(self.id));
        } else if self.request_focus {
            ui.ctx().memory_mut(|m| m.request_focus(self.id));
        }
        self.request_focus = false;

        // Inline embeds: pictures, drawings and PDFs are shown in place of their link unless the
        // cursor is inside the link.
        let mut images: Vec<(Range<usize>, f32, Art, Vec2)> = Vec::new();
        if self.inline_images {
            let focused = ui.ctx().memory(|m| m.has_focus(self.id));
            let sel = egui::text_edit::TextEditState::load(ui.ctx(), self.id).and_then(|st| st.cursor.char_range()).map(|r| {
                let a = markdown::char_to_byte(text, r.primary.index.0);
                let b = markdown::char_to_byte(text, r.secondary.index.0);
                a.min(b)..a.max(b)
            });
            let max_w = (ui.available_width() - 8.0).max(80.0);
            for (l, ok) in &self.cache.links {
                if !l.embed || !*ok {
                    continue;
                }
                let inside = focused && sel.as_ref().map(|s| s.start <= l.range.end && s.end >= l.range.start).unwrap_or(false);
                if inside {
                    continue;
                }
                let Some(p) = vault.resolve(&l.target, Some(rel)) else { continue };
                let abs = vault.abs(&p);
                let fit = |w: f32, h: f32, max_h: f32| {
                    let k = (max_w / w).min(max_h / h).min(1.0);
                    Vec2::new(w * k, h * k)
                };
                if crate::media::is_image(&p) {
                    let Some(tex) = crate::media::texture(ui.ctx(), &abs) else { continue };
                    let [tw, th] = tex.size_vec2().into();
                    let size = fit(tw, th, 420.0);
                    images.push((l.range.clone(), size.y + 6.0, Art::Tex(tex), size));
                } else if crate::excalidraw_md::is_drawing_path(&p) {
                    let Some(scene) = crate::reader::scene_cached(ui.ctx(), &abs) else { continue };
                    let Some(b) = scene.bounds() else { continue };
                    let b = b.expand(20.0);
                    let size = fit(b.width().max(40.0), b.height().max(40.0), 420.0);
                    images.push((l.range.clone(), size.y + 6.0, Art::Scene(scene, p.clone()), size));
                } else if crate::pdfview::is_pdf(&p) {
                    match crate::pdfview::pdf_pages(ui.ctx(), &abs, 1) {
                        crate::pdfview::PdfState::Ready { pages, total } if !pages.is_empty() => {
                            let [tw, th] = pages[0].size_vec2().into();
                            let size = fit(tw, th, 520.0);
                            images.push((l.range.clone(), size.y + 6.0, Art::Pdf(pages[0].clone(), total), size));
                        }
                        crate::pdfview::PdfState::Loading => ui.ctx().request_repaint_after(std::time::Duration::from_millis(100)),
                        _ => {}
                    }
                }
            }
        }
        // The embeds shown as pictures, by their link text (in order). When typing changes the
        // text in the middle of a frame, the pictures are found again in the new text, so they
        // stay pictures instead of flashing back to link text for a frame.
        let image_keys: Vec<(String, f32)> = images.iter().map(|(r, h, _, _)| (text[r.clone()].to_string(), *h)).collect();
        let links = self.cache.links.clone();
        let links_text = text.clone();
        self.assist.arm(text, media_handled);
        let mut layouter = |ui: &Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| -> Arc<Galley> {
            let s = buf.as_str();
            // The cached links belong to the text from the start of the frame; if the text box
            // changed it this frame, find them again (cheap for normal notes).
            let fresh;
            let use_links: &[(LinkSpan, bool)] = if s == links_text {
                &links
            } else {
                fresh = markdown::find_links(s)
                    .into_iter()
                    .map(|l| {
                        let ok = vault.exists(&l.target, Some(rel));
                        (l, ok)
                    })
                    .collect::<Vec<_>>();
                &fresh
            };
            let ranges: Vec<(Range<usize>, f32)> = remap_images(s, &image_keys).into_iter().map(|(r, h, _)| (r, h)).collect();
            let mut job = highlight_with_images(s, use_links, base_size, &ranges);
            job.wrap.max_width = wrap_width;
            ui.ctx().fonts_mut(|f| f.layout_job(job))
        };

        let output = egui::TextEdit::multiline(text)
            .id(self.id)
            .frame(egui::Frame::NONE)
            .desired_width(f32::INFINITY)
            .desired_rows(20)
            .event_filter(egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true })
            .layouter(&mut layouter)
            .show(ui);
        let resp = &output.response.response;
        if resp.changed() {
            changed = true;
        }

        self.last_layout = Some((output.galley.clone(), output.galley_pos));
        // paint the inline pictures over their (invisible) link text
        if !images.is_empty() {
            let painter = ui.painter().with_clip_rect(ui.clip_rect());
            let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
            let now_text = output.galley.job.text.clone();
            for (r, _, idx) in remap_images(&now_text, &image_keys) {
                let (_, _, art, size) = &images[idx];
                let c = output.galley.pos_from_cursor(CCursor::new(markdown::byte_to_char(&now_text, r.start)));
                let min = output.galley_pos + Vec2::new(c.min.x, c.min.y + 3.0);
                let rect = Rect::from_min_size(min, *size);
                match art {
                    Art::Tex(tex) => {
                        painter.image(tex.id(), rect, uv, Color32::WHITE);
                    }
                    Art::Scene(scene, at) => {
                        let bg = if self.dark_canvas { Color32::from_rgb(18, 18, 18) } else { Color32::WHITE };
                        painter.rect_filled(rect, 6.0, bg);
                        let pics = crate::drawing_pictures::textures(ui.ctx(), scene, vault, at);
                        crate::drawing::paint_thumbnail(&painter, scene, rect.shrink(4.0), self.dark_canvas, &pics);
                    }
                    Art::Pdf(tex, total) => {
                        painter.image(tex.id(), rect, uv, Color32::WHITE);
                        painter.rect_stroke(rect, 0.0, Stroke::new(1.0, theme::BORDER), egui::StrokeKind::Outside);
                        let label = if *total > 1 { format!("PDF · page 1 of {total}") } else { "PDF".to_string() };
                        let g = painter.layout_no_wrap(label, egui::FontId::proportional(11.0), Color32::WHITE);
                        let tag = Rect::from_min_size(rect.min + Vec2::new(6.0, 6.0), g.size() + Vec2::new(10.0, 4.0));
                        painter.rect_filled(tag, 4.0, Color32::from_black_alpha(170));
                        painter.galley(tag.min + Vec2::new(5.0, 2.0), g, Color32::WHITE);
                    }
                }
            }
        }
        // when the cursor moved into or out of an image link, lay out again next frame
        if self.inline_images {
            let now = output.cursor_range.map(|r| (r.primary.index.0, r.secondary.index.0));
            if now != self.last_cursor {
                self.last_cursor = now;
                ui.ctx().request_repaint();
            }
        }
        // normal mode: a block on the character under the cursor
        if self.vim_enabled && resp.has_focus() && self.vim.mode == crate::vim::Mode::Normal {
            let c = self.vim.cursor;
            let here = output.galley.pos_from_cursor(CCursor::new(c));
            let next = output.galley.pos_from_cursor(CCursor::new(c + 1));
            let w = if (next.min.y - here.min.y).abs() < 1.0 && next.min.x > here.min.x { next.min.x - here.min.x } else { base_size * 0.55 };
            let block = Rect::from_min_size(output.galley_pos + here.min.to_vec2(), Vec2::new(w, here.height()));
            ui.painter().rect_filled(block, 1.0, theme::ACCENT.gamma_multiply(0.45));
        }
        if let Some(c) = self.pending_cursor.take().or(self.vim_scroll.take()) {
            let r = output.galley.pos_from_cursor(CCursor::new(c));
            let screen = r.translate(output.galley_pos.to_vec2());
            ui.scroll_to_rect(screen.expand(60.0), Some(egui::Align::Center));
        }

        // --- hover / click on links and checkboxes
        let hover = ui.input(|i| i.pointer.hover_pos());
        let modifiers = ui.input(|i| i.modifiers);
        let follow_ok = !ctrl_to_follow || modifiers.command;
        if let Some(hp) = hover.filter(|p| resp.rect.contains(*p)) {
            if let Some(byte) = char_under(&output.galley, output.galley_pos, hp).map(|c| markdown::char_to_byte(text, c)) {
                let link = self.cache.links.iter().find(|(l, _)| l.range.contains(&byte) && !images.iter().any(|(r, ..)| *r == l.range)).cloned();
                let web = if link.is_none() { crate::lists::find_web_links(text).into_iter().find(|w| w.range.contains(&byte)) } else { None };
                if let Some(w) = &web {
                    if follow_ok {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    let hint = if ctrl_to_follow { "Ctrl+click to open in your browser" } else { "Click to open in your browser · Ctrl+click to edit" };
                    resp.clone().on_hover_ui_at_pointer(|ui| {
                        ui.label(egui::RichText::new(&w.url).color(theme::LINK));
                        ui.label(egui::RichText::new(hint).small().color(theme::MUTED));
                    });
                }
                if let Some((l, exists)) = &link {
                    if follow_ok {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    let tip = if *exists { format!("Open “{}”", l.target) } else { format!("Create note “{}”", l.target) };
                    let hint = if ctrl_to_follow { "Ctrl+click to follow" } else { "Ctrl+click to edit the link text" };
                    let colour = if *exists { theme::LINK } else { theme::LINK_MISSING };
                    let image = vault.resolve(&l.target, Some(rel)).filter(|p| crate::media::is_image(p)).map(|p| vault.abs(&p));
                    // several files share the name, or a red path has several candidates (LINK-008, LINK-012)
                    let note = vault.link_hint(&l.target, Some(rel));
                    resp.clone().on_hover_ui_at_pointer(|ui| {
                        if let Some(img) = image.as_ref().and_then(|p| crate::media::image_widget(ui.ctx(), p, 320.0, 240.0)) {
                            ui.add(img);
                        }
                        ui.label(egui::RichText::new(tip).color(colour));
                        if let Some(n) = &note {
                            ui.label(egui::RichText::new(n).small().color(theme::TEXT));
                        }
                        ui.label(egui::RichText::new(hint).small().color(theme::MUTED));
                    });
                }
                if let (true, Some(web)) = (resp.clicked(), web.as_ref()) {
                    let follow = if ctrl_to_follow { modifiers.command } else { !modifiers.command };
                    if follow {
                        action = Some(EditorAction::OpenUrl(web.url.clone()));
                    }
                } else if resp.clicked() {
                    if let Some((l, _)) = link {
                        // Plain click follows (unless the setting asks for Ctrl); with the default
                        // setting, Ctrl+click just places the cursor so the link can be edited.
                        let follow = if ctrl_to_follow { modifiers.command } else { !modifiers.command };
                        if follow {
                            action = Some(EditorAction::OpenLink { target: l.target.clone(), heading: l.heading.clone(), new_tab: modifiers.shift || ui.input(|i| i.pointer.middle_down()) });
                        }
                    } else if let Some(t) = markdown::find_tasks(text).into_iter().find(|t| byte + 1 >= t.check_offset && byte <= t.check_offset + 1) {
                        if markdown::toggle_task_at(text, t.check_offset) {
                            changed = true;
                        }
                    }
                }
            }
        }
        if resp.middle_clicked() {
            if let Some(hp) = hover {
                if let Some(byte) = char_under(&output.galley, output.galley_pos, hp).map(|c| markdown::char_to_byte(text, c)) {
                    if let Some((l, _)) = self.cache.links.iter().find(|(l, _)| l.range.contains(&byte)) {
                        action = Some(EditorAction::OpenLink { target: l.target.clone(), heading: l.heading.clone(), new_tab: true });
                    }
                }
            }
        }

        let (c, a) = self.assist.after(ui, text, vault, &output, self.typing(), media_handled);
        changed |= c;
        if a.is_some() {
            action = a;
        }
        (changed, action)
    }
}

/// Find the embeds that are shown as pictures again in (possibly edited) text: walks the embed
/// links of `text` in order and matches them with `keys` (link text, row height) in order.
/// Returns (byte range in `text`, row height, index into `keys`).
fn remap_images(text: &str, keys: &[(String, f32)]) -> Vec<(Range<usize>, f32, usize)> {
    if keys.is_empty() {
        return vec![];
    }
    let mut out = Vec::new();
    let mut j = 0;
    for l in markdown::find_links(text).into_iter().filter(|l| l.embed) {
        if j >= keys.len() {
            break;
        }
        let here = &text[l.range.clone()];
        // skip keys whose link was removed by the edit
        if let Some(k) = keys[j..].iter().position(|(t, _)| t == here) {
            j += k;
            out.push((l.range.clone(), keys[j].1, j));
            j += 1;
        }
    }
    out
}

/// The character index whose glyph is under `p`, if the pointer is really over text.
pub fn char_under(galley: &Galley, galley_pos: Pos2, p: Pos2) -> Option<usize> {
    let local = p - galley_pos;
    let c = galley.cursor_from_pos(local);
    let idx = c.index.0;
    for k in [idx, idx.saturating_sub(1)] {
        let a = galley.pos_from_cursor(CCursor::new(k));
        let b = galley.pos_from_cursor(CCursor::new(k + 1));
        if (a.min.y - b.min.y).abs() > 0.5 {
            continue;
        }
        let r = Rect::from_min_max(Pos2::new(a.min.x.min(b.min.x), a.min.y), Pos2::new(a.min.x.max(b.min.x), a.max.y));
        if r.expand2(Vec2::new(0.5, 0.0)).contains(local.to_pos2()) {
            return Some(k);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, PointerButton, RawInput};

    struct Harness {
        ctx: egui::Context,
        time: f64,
    }

    impl Harness {
        fn frame(&mut self, ed: &mut NoteEditor, text: &mut String, vault: &Vault, rel: &Path, events: Vec<Event>) -> (bool, Option<EditorAction>) {
            self.time += 0.05;
            let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 700.0))), time: Some(self.time), events, ..Default::default() };
            let mut changed = false;
            let mut action = None;
            let mut out = self.ctx.run_ui(raw, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    // another focusable widget, like the real app's title box and sidebars
                    let mut other = String::from("title");
                    ui.add(egui::TextEdit::singleline(&mut other).id(egui::Id::unique("other-widget")));
                    let (c, a) = ed.show(ui, text, vault, rel, 15.0, false);
                    let mut other2 = String::from("after");
                    ui.add(egui::TextEdit::singleline(&mut other2).id(egui::Id::unique("other-widget-2")));
                    changed |= c;
                    if a.is_some() {
                        action = a;
                    }
                });
            });
            out.textures_delta.clear();
            (changed, action)
        }
    }

    fn click_events(p: Pos2, pressed: bool) -> Vec<Event> {
        vec![Event::PointerMoved(p), Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }]
    }

    fn vault_for_tests(tag: &str) -> (Vault, std::path::PathBuf) {
        let d = std::env::temp_dir().join(format!("shale-ed-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        crate::vault::write_sample_vault(&d).unwrap();
        (Vault::open(&d).unwrap(), d)
    }

    fn click_word(h: &mut Harness, ed: &mut NoteEditor, text: &mut String, v: &Vault, rel: &Path, word: &str) -> Option<EditorAction> {
        let (galley, pos) = ed.last_layout.clone().unwrap();
        let ch = markdown::byte_to_char(text, text.find(word).unwrap()) + 1;
        let p = pos + galley.pos_from_cursor(CCursor::new(ch)).center().to_vec2() + Vec2::new(3.0, 0.0);
        h.frame(ed, text, v, rel, click_events(p, true));
        let (_, a) = h.frame(ed, text, v, rel, click_events(p, false));
        a
    }

    /// Run one frame of the editor alone; returns what it copied and whether it asked for a paste.
    fn vim_frame(ctx: &egui::Context, t: &mut f64, ed: &mut NoteEditor, text: &mut String, v: &Vault, events: Vec<Event>) -> (Option<String>, bool) {
        *t += 0.05;
        let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 700.0))), time: Some(*t), events, ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                ed.show(ui, text, v, Path::new("Welcome.md"), 15.0, false);
            });
        });
        out.textures_delta.clear();
        let copied = out.platform_output.commands.iter().find_map(|c| match c {
            egui::OutputCommand::CopyText(s) => Some(s.clone()),
            _ => None,
        });
        let paste = out.viewport_output.values().any(|vo| vo.commands.iter().any(|c| matches!(c, egui::ViewportCommand::RequestPaste)));
        (copied, paste)
    }

    // spec: EDIT-010, EDIT-011, EDIT-012, EDIT-013, EDIT-014
    #[test]
    fn vim_lite_in_the_real_editor() {
        let (v, d) = vault_for_tests("vim");
        let ctx = egui::Context::default();
        let mut t = 0.0;
        let mut ed = NoteEditor::new(egui::Id::unique("vim-ed"));
        ed.vim_enabled = true;
        let mut text = String::from("alpha beta\ngamma delta\nsee [[Ide");
        let cursor = |ctx: &egui::Context, ed: &NoteEditor| egui::text_edit::TextEditState::load(ctx, ed.id).and_then(|s| s.cursor.char_range()).map(|r| r.primary.index.0).unwrap();
        let typed = |s: &str| vec![Event::Text(s.into())];
        let key = |k: Key| vec![Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }];
        ed.pending_cursor = Some(0);
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, vec![]);
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, vec![]);
        assert_eq!(ed.vim_status().as_deref(), Some("NORMAL"), "a note that gets the keyboard is in normal mode");
        // motions move the text box's cursor and type nothing
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, typed("w"));
        assert_eq!(cursor(&ctx, &ed), 6);
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, typed("j"));
        assert_eq!(cursor(&ctx, &ed), 17, "same column on the next line");
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, key(Key::ArrowUp));
        assert_eq!(cursor(&ctx, &ed), 6, "arrow keys are motions too");
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, key(Key::Enter));
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, key(Key::Backspace));
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, typed("q z"));
        assert_eq!(text, "alpha beta\ngamma delta\nsee [[Ide", "no key types in normal mode");
        assert_eq!(cursor(&ctx, &ed), 16, "Enter = j, Backspace = h");
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, typed("l"));
        // i: typing; Esc: back to normal on the typed character
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, typed("i"));
        assert_eq!(ed.vim_status().as_deref(), Some("INSERT"));
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, typed("X"));
        assert_eq!(text, "alpha beta\ngamma Xdelta\nsee [[Ide");
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, key(Key::Escape));
        assert_eq!((ed.vim_status().as_deref(), cursor(&ctx, &ed)), (Some("NORMAL"), 17));
        // visual + d: the selection goes to the clipboard
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, typed("0"));
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, typed("v"));
        assert_eq!(ed.vim_status().as_deref(), Some("VISUAL"));
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, typed("w"));
        let (copied, _) = vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, typed("d"));
        assert_eq!(copied.as_deref(), Some("gamma X"));
        assert_eq!(text, "alpha beta\ndelta\nsee [[Ide");
        // p asks the window for the clipboard and puts what arrives after the cursor
        let (_, asked) = vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, typed("p"));
        assert!(asked, "p sends RequestPaste");
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, vec![Event::Paste("ZZ".into())]);
        assert_eq!(text, "alpha beta\ndZZelta\nsee [[Ide");
        // :line and /search
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, typed(":3"));
        assert_eq!(ed.vim_status().as_deref(), Some(":3"), "the command shows while it is typed");
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, key(Key::Enter));
        assert_eq!(cursor(&ctx, &ed), 19, "start of line 3 (\"see [[Ide\")");
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, typed("/BETA"));
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, key(Key::Enter));
        assert_eq!(cursor(&ctx, &ed), 6);
        // the [[ suggestions stay closed in normal mode and open while typing
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, typed("G$"));
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, vec![]);
        assert!(!ed.assist.ac_active, "no suggestions in normal mode, although the cursor is in [[Ide");
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, typed("A"));
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, vec![]);
        assert!(ed.assist.ac_active, "insert mode: the suggestions open");
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: LINK-005
    #[test]
    fn clicking_links_in_the_editor() {
        let (v, d) = vault_for_tests("click");
        let rel = Path::new("Welcome.md");
        let mut text = String::from("Go to [[Ideas]] or make [[Brand new]] now");
        let mut ed = NoteEditor::new(egui::Id::unique("t"));
        let mut h = Harness { ctx: egui::Context::default(), time: 0.0 };
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        match click_word(&mut h, &mut ed, &mut text, &v, rel, "Ideas") {
            Some(EditorAction::OpenLink { target, .. }) => assert_eq!(target, "Ideas"),
            _ => panic!("clicking a blue link did nothing"),
        }
        match click_word(&mut h, &mut ed, &mut text, &v, rel, "Brand new") {
            Some(EditorAction::OpenLink { target, .. }) => assert_eq!(target, "Brand new"),
            _ => panic!("clicking a red link did nothing"),
        }
        // clicking plain text does not follow anything
        assert!(click_word(&mut h, &mut ed, &mut text, &v, rel, "make").is_none());
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: EDIT-007
    #[test]
    fn clicking_a_checkbox_in_the_editor_ticks_it() {
        let (v, d) = vault_for_tests("check");
        let rel = Path::new("Welcome.md");
        let mut text = String::from("- [ ] buy milk");
        let mut ed = NoteEditor::new(egui::Id::unique("t2"));
        let mut h = Harness { ctx: egui::Context::default(), time: 0.0 };
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        let (galley, pos) = ed.last_layout.clone().unwrap();
        let p = pos + galley.pos_from_cursor(CCursor::new(3)).center().to_vec2() + Vec2::new(2.0, 0.0);
        h.frame(&mut ed, &mut text, &v, rel, click_events(p, true));
        h.frame(&mut ed, &mut text, &v, rel, click_events(p, false));
        assert_eq!(text, "- [x] buy milk");
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: EDIT-005
    #[test]
    fn enter_and_tab_keep_lists_going() {
        let (v, d) = vault_for_tests("lists");
        let rel = Path::new("Welcome.md");
        let mut text = String::new();
        let mut ed = NoteEditor::new(egui::Id::unique("t4"));
        ed.request_focus = true;
        let mut h = Harness { ctx: egui::Context::default(), time: 0.0 };
        let key = |key: Key, modifiers: Modifiers| Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers };
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("1. [ ] buy milk".into())]);
        h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::Enter, Modifiers::NONE)]);
        assert_eq!(text, "1. [ ] buy milk\n2. [ ] ");
        h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::Tab, Modifiers::NONE)]);
        assert_eq!(text, "1. [ ] buy milk\n    1. [ ] ");
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("oat".into())]);
        assert_eq!(text, "1. [ ] buy milk\n    1. [ ] oat");
        h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::Tab, Modifiers::SHIFT)]);
        assert_eq!(text, "1. [ ] buy milk\n2. [ ] oat");
        // empty item + Enter ends the list
        h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::Enter, Modifiers::NONE)]);
        h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::Enter, Modifiers::NONE)]);
        assert_eq!(text, "1. [ ] buy milk\n2. [ ] oat\n");
        // headings: Tab adds a level, Shift+Tab removes one
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("# Title".into())]);
        h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::Tab, Modifiers::NONE)]);
        assert!(text.ends_with("\n## Title"));
        h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::Tab, Modifiers::SHIFT)]);
        assert!(text.ends_with("\n# Title"));
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: EDIT-005
    #[test]
    fn alt_arrows_move_lines() {
        let (v, d) = vault_for_tests("move");
        let rel = Path::new("Welcome.md");
        let mut text = String::new();
        let mut ed = NoteEditor::new(egui::Id::unique("t6"));
        ed.request_focus = true;
        let mut h = Harness { ctx: egui::Context::default(), time: 0.0 };
        let key = |key: Key, modifiers: Modifiers| Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers };
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("- a\n    - b".into())]);
        h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::ArrowUp, Modifiers::ALT)]);
        assert_eq!(text, "- b\n- a");
        h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::ArrowDown, Modifiers::ALT)]);
        assert_eq!(text, "- a\n- b");
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: EDIT-002
    #[test]
    fn file_picker_completes_paths_and_links_the_file() {
        let (v, d) = vault_for_tests("picker");
        let rel = Path::new("Welcome.md");
        let mut text = String::new();
        let mut ed = NoteEditor::new(egui::Id::unique("t7"));
        ed.request_focus = true;
        let mut h = Harness { ctx: egui::Context::default(), time: 0.0 };
        let key = |key: Key| Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("pic: [!/Pro".into())]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::Tab)]);
        assert_eq!(text, "pic: [!/Projects/");
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("G".into())]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::Tab)]);
        assert_eq!(text, "pic: [!/Projects/Garden plan.md");
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        let (_, a) = h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::Enter)]);
        assert_eq!(text, "pic: [[Projects/Garden plan.md]]");
        assert!(matches!(a, Some(EditorAction::FilesChanged)));
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: EDIT-002
    #[test]
    fn file_picker_arrows_and_tab_keep_working() {
        let (v, d) = vault_for_tests("picker2");
        let rel = Path::new("Welcome.md");
        let mut text = String::new();
        let mut ed = NoteEditor::new(egui::Id::unique("t8"));
        ed.request_focus = true;
        let mut h = Harness { ctx: egui::Context::default(), time: 0.0 };
        let key = |key: Key| Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("[!/".into())]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert!(ed.assist.media.items.len() > 2, "items: {:?}", ed.assist.media.items);
        h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::ArrowDown)]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert_eq!(ed.assist.media.sel, 1, "arrow down should move the selection");
        h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::ArrowUp)]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert_eq!(ed.assist.media.sel, 0);
        // Tab with several matches and nothing in common: the highlighted entry goes into the path,
        // the picker stays open and the editor keeps focus
        h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::Tab)]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert_eq!(text, "[!/Projects/");
        assert!(ed.assist.media.active, "picker should stay open after Tab");
        assert!(h.ctx.memory(|m| m.has_focus(ed.id)), "editor should keep focus after Tab");
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: EDIT-006
    #[test]
    fn inline_images_show_until_the_cursor_enters_the_link() {
        let (v, d) = vault_for_tests("inline");
        image::RgbaImage::from_pixel(200, 120, image::Rgba([200, 30, 30, 255])).save(d.join("pic.png")).unwrap();
        let mut v = v;
        v.rescan();
        let rel = Path::new("Welcome.md");
        let mut text = String::from("intro\n![[pic.png]]\nafter");
        let mut ed = NoteEditor::new(egui::Id::unique("t9"));
        ed.inline_images = true;
        let mut h = Harness { ctx: egui::Context::default(), time: 0.0 };
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        let tall = ed.last_layout.as_ref().unwrap().0.size().y;
        assert!(tall > 120.0, "image row should make room for the picture, height {tall}");
        // put the cursor inside the link: the text comes back, the row is normal again
        ed.pending_cursor = Some(9);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        let short = ed.last_layout.as_ref().unwrap().0.size().y;
        assert!(short < 100.0, "with the cursor in the link it should be plain text, height {short}");
        assert_eq!(text, "intro\n![[pic.png]]\nafter");
        // setting off: never a picture
        let mut ed2 = NoteEditor::new(egui::Id::unique("t10"));
        h.frame(&mut ed2, &mut text, &v, rel, vec![]);
        assert!(ed2.last_layout.as_ref().unwrap().0.size().y < 100.0);
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: EDIT-002
    #[test]
    fn escape_closes_the_file_picker() {
        let (v, d) = vault_for_tests("esc");
        let rel = Path::new("Welcome.md");
        let mut text = String::new();
        let mut ed = NoteEditor::new(egui::Id::unique("t11"));
        ed.request_focus = true;
        let mut h = Harness { ctx: egui::Context::default(), time: 0.0 };
        let key = |key: Key| Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("[!/".into())]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert!(ed.assist.media.active);
        h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::Escape)]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert!(!ed.assist.media.active, "Esc should close the picker");
        assert!(!ed.picker_visible(), "the popup should not be drawn");
        assert_eq!(text, "[!/");
        // losing focus for a moment (e.g. clicking elsewhere) doesn't bring it back
        h.ctx.memory_mut(|m| m.surrender_focus(ed.id));
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.ctx.memory_mut(|m| m.request_focus(ed.id));
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert!(!ed.picker_visible(), "a closed picker should stay closed");
        // typing on keeps it closed for this [! ...
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("P".into())]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert!(!ed.picker_visible());
        // ... but a new [! opens it again
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text(" [!/".into())]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert!(ed.picker_visible());
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: EDIT-002
    #[test]
    fn picker_safety_net_when_keys_reach_the_text_box() {
        // On some desktops the keys arrive as plain typed text instead of key presses.
        // Then a tab or newline lands in the text: the picker must take it back and do its job.
        let (v, d) = vault_for_tests("net");
        let rel = Path::new("Welcome.md");
        let mut text = String::from("start\n");
        let mut ed = NoteEditor::new(egui::Id::unique("t12"));
        ed.pending_cursor = Some(6);
        let mut h = Harness { ctx: egui::Context::default(), time: 0.0 };
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("[!/Pro".into())]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert!(ed.picker_visible());
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("\t".into())]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert_eq!(text, "start\n[!/Projects/", "a typed tab should complete, not stay in the text");
        assert!(ed.picker_visible());
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("\t".into())]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert_eq!(text, "start\n[!/Projects/Garden plan.md", "the only entry gets completed");
        // a second tab with nothing left to complete changes nothing and doesn't finish the link
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("\t".into())]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert_eq!(text, "start\n[!/Projects/Garden plan.md");
        assert!(ed.picker_visible());
        // Enter picks the file
        let enter = Event::Key { key: Key::Enter, physical_key: Some(Key::Enter), pressed: true, repeat: false, modifiers: Modifiers::NONE };
        h.frame(&mut ed, &mut text, &v, rel, vec![enter]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert_eq!(text, "start\n[[Projects/Garden plan.md]]");
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: EDIT-002
    #[test]
    fn double_bracket_bang_opens_only_the_file_picker() {
        // `[[!` must not also open the note suggestions (they took the keys away from the picker)
        assert_eq!(link_query("[[!../x", 7), None);
        assert_eq!(link_query("[[!", 3), None);
        let (v, d) = vault_for_tests("bb");
        let rel = Path::new("Welcome.md");
        let mut text = String::new();
        let mut ed = NoteEditor::new(egui::Id::unique("t13"));
        ed.request_focus = true;
        let mut h = Harness { ctx: egui::Context::default(), time: 0.0 };
        let key = |key: Key| Event::Key { key, physical_key: Some(key), pressed: true, repeat: false, modifiers: Modifiers::NONE };
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("[[!/".into())]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert!(ed.picker_visible());
        assert!(!ed.assist.ac_active, "note suggestions must stay closed for [[!");
        h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::ArrowDown)]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert_eq!(ed.assist.media.sel, 1);
        h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::ArrowUp)]);
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("Pro".into())]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::Tab)]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert_eq!(text, "[[!/Projects/");
        h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::Tab)]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert_eq!(text, "[[!/Projects/Garden plan.md");
        h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::Enter)]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert_eq!(text, "[[Projects/Garden plan.md]]");
        // Esc closes it
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text(" [[!/".into())]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert!(ed.picker_visible());
        h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::Escape)]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert!(!ed.picker_visible());
        assert!(!ed.assist.ac_active);
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: EDIT-006
    #[test]
    fn drawings_and_pdfs_show_inline_like_pictures() {
        let (v, d) = vault_for_tests("inline-art");
        std::fs::write(d.join("doc.pdf"), crate::pdfview::sample_pdf()).unwrap();
        let mut v = v;
        v.rescan();
        let rel = Path::new("Welcome.md");
        let mut h = Harness { ctx: egui::Context::default(), time: 0.0 };
        crate::theme::install_fonts(&h.ctx);
        for (text, what) in [("intro\n![[Sketch.excalidraw]]\nafter", "drawing"), ("intro\n![[doc.pdf]]\nafter", "pdf")] {
            let mut text = text.to_string();
            let mut ed = NoteEditor::new(egui::Id::unique(("inline-art", what)));
            ed.inline_images = true;
            let mut tall = 0.0;
            for _ in 0..200 {
                h.frame(&mut ed, &mut text, &v, rel, vec![]);
                tall = ed.last_layout.as_ref().unwrap().0.size().y;
                if tall > 120.0 {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10)); // PDFs render in the background
            }
            assert!(tall > 120.0, "{what} is shown inline, height {tall}");
            // with the cursor in the link, the link text is back
            ed.pending_cursor = Some(9);
            for _ in 0..3 {
                h.frame(&mut ed, &mut text, &v, rel, vec![]);
            }
            assert!(ed.last_layout.as_ref().unwrap().0.size().y < 100.0, "{what}: link text while editing it");
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: EDIT-006
    #[test]
    fn typing_keeps_pictures_in_place() {
        // regression: on every keypress the picture became link text for one frame, so it
        // flickered and the view jumped
        let (v, d) = vault_for_tests("noflicker");
        image::RgbaImage::from_pixel(200, 120, image::Rgba([200, 30, 30, 255])).save(d.join("pic.png")).unwrap();
        let mut v = v;
        v.rescan();
        let rel = Path::new("Welcome.md");
        let mut text = String::from("intro\n![[pic.png]]\nafter");
        let mut ed = NoteEditor::new(egui::Id::unique("t-noflicker"));
        ed.inline_images = true;
        ed.pending_cursor = Some(text.chars().count()); // cursor at the end, outside the link
        let mut h = Harness { ctx: egui::Context::default(), time: 0.0 };
        for _ in 0..3 {
            h.frame(&mut ed, &mut text, &v, rel, vec![]);
        }
        let tall = ed.last_layout.as_ref().unwrap().0.size().y;
        assert!(tall > 120.0);
        for (i, ch) in ["x", "y", "z"].iter().enumerate() {
            h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text(ch.to_string())]);
            let now = ed.last_layout.as_ref().unwrap().0.size().y;
            assert!(now > 120.0, "keypress {i}: the picture must stay a picture in the frame of the keypress (height {now})");
        }
        assert_eq!(text, "intro\n![[pic.png]]\nafterxyz");
        // typing in front of the picture moves it, it still stays a picture
        ed.pending_cursor = Some(0);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("Q".into())]);
        assert!(ed.last_layout.as_ref().unwrap().0.size().y > 120.0);
        assert!(text.starts_with("Qintro"));
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: EDIT-006
    #[test]
    fn remapping_follows_edits() {
        let keys = vec![("![[a.png]]".to_string(), 50.0), ("![[b.png]]".to_string(), 60.0)];
        let t = "x ![[a.png]] y ![[b.png]]";
        let r = remap_images(t, &keys);
        assert_eq!(r.len(), 2);
        assert_eq!(&t[r[1].0.clone()], "![[b.png]]");
        let t2 = "NEW TEXT x ![[a.png]] y ![[b.png]]";
        let r2 = remap_images(t2, &keys);
        assert_eq!(r2[0].0.start, 11);
        assert_eq!(r2[1].2, 1);
        // an embed removed by the edit is skipped, the rest still match
        let r3 = remap_images("only ![[b.png]]", &keys);
        assert_eq!(r3.len(), 1);
        assert_eq!(r3[0].2, 1);
        // a link that was shown as text (cursor inside) is not a key and stays text
        assert!(remap_images("![[c.png]]", &keys).is_empty());
    }

    // spec: EDIT-001
    #[test]
    fn escape_closes_the_note_suggestions() {
        let (v, d) = vault_for_tests("acesc");
        let rel = Path::new("Welcome.md");
        let mut text = String::new();
        let mut ed = NoteEditor::new(egui::Id::unique("t-acesc"));
        ed.request_focus = true;
        let mut h = Harness { ctx: egui::Context::default(), time: 0.0 };
        let key = |key: Key| Event::Key { key, physical_key: Some(key), pressed: true, repeat: false, modifiers: Modifiers::NONE };
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("see [[Ide".into())]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert!(ed.assist.ac_active, "suggestions open");
        h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::Escape)]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert!(!ed.assist.ac_active, "Esc closes the note suggestions");
        assert_eq!(text, "see [[Ide", "text unchanged");
        // typing on keeps them closed for this [[
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("a".into())]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert!(!ed.assist.ac_active);
        // Enter after Esc is a normal new line, not a pick
        h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::Enter)]);
        assert!(!text.contains("]]"), "nothing was picked: {text:?}");
        // a new [[ opens them again
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text(" [[We".into())]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert!(ed.assist.ac_active, "a new [[ opens the suggestions");
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: EDIT-009
    #[test]
    fn clicking_a_web_link_opens_the_browser_action() {
        let (v, d) = vault_for_tests("web");
        let rel = Path::new("Welcome.md");
        let mut text = String::from("see [this is google](https://google.com) ok");
        let mut ed = NoteEditor::new(egui::Id::unique("t5"));
        let mut h = Harness { ctx: egui::Context::default(), time: 0.0 };
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        match click_word(&mut h, &mut ed, &mut text, &v, rel, "google]") {
            Some(EditorAction::OpenUrl(u)) => assert_eq!(u, "https://google.com"),
            _ => panic!("clicking a web link should open it"),
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: EDIT-001
    #[test]
    fn typing_double_brackets_suggests_and_completes() {
        let (v, d) = vault_for_tests("ac");
        let rel = Path::new("Welcome.md");
        let mut text = String::new();
        let mut ed = NoteEditor::new(egui::Id::unique("t3"));
        ed.request_focus = true;
        let mut h = Harness { ctx: egui::Context::default(), time: 0.0 };
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("See [[Ide".into())]);
        assert_eq!(text, "See [[Ide");
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        let enter = Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
        h.frame(&mut ed, &mut text, &v, rel, vec![enter]);
        assert_eq!(text, "See [[Ideas]]");
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: PPL-001, EDIT-004
    #[test]
    fn double_brackets_p_lists_people() {
        let (mut v, d) = vault_for_tests("people");
        std::fs::create_dir_all(d.join("Adressbuch")).unwrap();
        std::fs::write(d.join("Adressbuch/Hans Müller.md"), "# Hans Müller\n").unwrap();
        std::fs::write(d.join("Adressbuch/Anna Schmidt.md"), "# Anna\n").unwrap();
        std::fs::write(d.join("Müllabfuhr.md"), "not a person\n").unwrap();
        v.rescan();
        let rel = Path::new("Welcome.md");
        let enter = Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
        let mut text = String::new();
        let mut ed = NoteEditor::new(egui::Id::unique("tp"));
        ed.request_focus = true;
        let mut h = Harness { ctx: egui::Context::default(), time: 0.0 };
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("Met [[p Mü".into())]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert_eq!(ed.assist.ac_labels, vec!["Hans Müller".to_string(), "＋ New contact “Mü” in Adressbuch".into()], "only the people folder");
        h.frame(&mut ed, &mut text, &v, rel, vec![enter.clone()]);
        assert_eq!(text, "Met [[Hans Müller]]", "a normal link (the p is only for the list)");
        // a new person: the link plus a request to make the contact note
        let mut text = String::new();
        let mut ed = NoteEditor::new(egui::Id::unique("tp2"));
        ed.request_focus = true;
        let mut h = Harness { ctx: egui::Context::default(), time: 0.0 };
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("[[p Jörg Weiß".into())]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        let (_, act) = h.frame(&mut ed, &mut text, &v, rel, vec![enter]);
        assert_eq!(text, "[[Jörg Weiß]]");
        assert!(matches!(act, Some(EditorAction::CreatePerson(n)) if n == "Jörg Weiß"));
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: EDIT-001
    #[test]
    fn link_query_detects_open_brackets() {
        let t = "hello [[Wor";
        assert_eq!(link_query(t, t.chars().count()), Some((8, "Wor".to_string())));
        let t2 = "hello [[World]] x";
        assert_eq!(link_query(t2, t2.chars().count()), None);
        let t3 = "a [[b|alias";
        assert_eq!(link_query(t3, t3.chars().count()), None);
        let t4 = "[[";
        assert_eq!(link_query(t4, 2), Some((2, String::new())));
    }

    // spec: LINK-004
    #[test]
    fn highlight_colours_links() {
        let text = "See [[Yes]] and [[No]]";
        let links: Vec<(LinkSpan, bool)> = markdown::find_links(text)
            .into_iter()
            .map(|l| {
                let ok = l.target == "Yes";
                (l, ok)
            })
            .collect();
        let job = highlight(text, &links, 15.0);
        let color_at = |byte: usize| job.sections.iter().find(|s| s.byte_range.start.0 <= byte && byte < s.byte_range.end.0).unwrap().format.color;
        assert_eq!(color_at(text.find("Yes").unwrap()), theme::LINK);
        assert_eq!(color_at(text.find("No").unwrap()), theme::LINK_MISSING);
        assert_eq!(color_at(0), theme::TEXT);
        // sections cover the whole text without gaps
        let mut pos = 0;
        for s in &job.sections {
            assert_eq!(s.byte_range.start.0, pos);
            pos = s.byte_range.end.0;
        }
        assert_eq!(pos, text.len());
    }

    // spec: LINK-004
    #[test]
    fn highlight_handles_unicode_and_headings() {
        let text = "# Überschrift\n- [x] erledigt **fett** `code`\n```\n[[nolink]]\n```";
        let links: Vec<(LinkSpan, bool)> = markdown::find_links(text).into_iter().map(|l| (l, true)).collect();
        assert!(links.is_empty());
        let job = highlight(text, &links, 15.0);
        assert!(job.sections[0].format.font_id.size > 20.0);
        let mut pos = 0;
        for s in &job.sections {
            assert_eq!(s.byte_range.start.0, pos);
            assert!(text.is_char_boundary(s.byte_range.end.0));
            pos = s.byte_range.end.0;
        }
    }
}
