//! The link preview of vim-lite normal mode (EDIT-033…036, raba/shale#250): a floating frame, like
//! neovim's hover, that shows where the link under the cursor leads.
//!
//! This file holds what needs no window: the link under the cursor ([`target_at`]), the web page
//! reader ([`parse_page`], [`fetch_page`]), the preview's state ([`LinkPreview`]) with its keys
//! ([`LinkPreview::keys`]) and the drawing of its content ([`LinkPreview::show`]). The app side
//! (opening it for the cursor, closing it when the cursor moves, carrying out an [`Outcome`]) is
//! `app/link_preview.rs`.

use crate::calendar::dav;
use crate::editor::EditorAction;
use crate::lists;
use crate::markdown;
use crate::reader;
use crate::theme;
use crate::vault::{FileKind, Vault};
use eframe::egui::{self, Event, InputState, Key, Pos2, Rect, Ui, Vec2};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;

/// The size a preview has by default and the least it can be dragged to (points, EDIT-037).
pub const DEFAULT_WIDTH: u32 = 640;
pub const DEFAULT_HEIGHT: u32 = 480;
pub const MIN_WIDTH: f32 = 240.0;
pub const MIN_HEIGHT: f32 = 160.0;
/// The room the frame keeps to the window's edges (points).
const SCREEN_MARGIN: f32 = 16.0;
/// How far the handles of the edges reach into and out of the frame (points).
pub const GRAB: f32 = 4.0;

/// The size of a preview for the setting `want` (EDIT-037): not under the least, not over the window.
/// A window smaller than the least wins over the least.
pub fn clamp_size(want: Vec2, screen: Vec2) -> Vec2 {
    let one = |w: f32, min: f32, room: f32| {
        let max = (room - SCREEN_MARGIN).max(1.0);
        w.clamp(min.min(max), max)
    };
    Vec2::new(one(want.x, MIN_WIDTH, screen.x), one(want.y, MIN_HEIGHT, screen.y))
}

/// The size in the settings (EDIT-037) as points.
pub fn size_of(cfg: &crate::config::Config) -> Vec2 {
    Vec2::new(cfg.link_preview_width as f32, cfg.link_preview_height as f32)
}

/// Which edges of the frame a handle drags.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Grip {
    /// -1 the left edge, 1 the right edge, 0 none
    pub x: i8,
    /// -1 the top edge, 1 the bottom edge, 0 none
    pub y: i8,
}

impl Grip {
    pub const ALL: [Grip; 8] = [Grip { x: -1, y: 0 }, Grip { x: 1, y: 0 }, Grip { x: 0, y: -1 }, Grip { x: 0, y: 1 }, Grip { x: -1, y: -1 }, Grip { x: 1, y: -1 }, Grip { x: -1, y: 1 }, Grip { x: 1, y: 1 }];

    /// The part of the frame `r` the handle covers: a strip along an edge, a square at a corner.
    pub fn rect(self, r: Rect) -> Rect {
        let one = |g: i8, min: f32, max: f32| match g {
            -1 => (min - GRAB, min + GRAB),
            1 => (max - GRAB, max + GRAB),
            _ => (min + GRAB, max - GRAB),
        };
        let (x0, x1) = one(self.x, r.min.x, r.max.x);
        let (y0, y1) = one(self.y, r.min.y, r.max.y);
        let (x1, y1) = (x1.max(x0), y1.max(y0));
        Rect::from_min_max(Pos2::new(x0, y0), Pos2::new(x1, y1))
    }

    /// The mouse cursor over the handle.
    pub fn cursor(self) -> egui::CursorIcon {
        match (self.x, self.y) {
            (0, _) => egui::CursorIcon::ResizeVertical,
            (_, 0) => egui::CursorIcon::ResizeHorizontal,
            (x, y) if x == y => egui::CursorIcon::ResizeNwSe,
            _ => egui::CursorIcon::ResizeNeSw,
        }
    }
}
/// The most of a note that is drawn (characters, cut at a line end).
const MAX_NOTE_CHARS: usize = 30_000;
/// The most that is downloaded of a web page.
const MAX_PAGE_BYTES: u64 = 1_500_000;
/// What is kept of the readable text of a web page.
const MAX_TEXT_CHARS: usize = 2_500;
const MAX_PARAGRAPHS: usize = 8;
/// Points a `j` / `k` scrolls.
const LINE: f32 = 48.0;

// ---------------------------------------------------------------------------------------------
// The link under the cursor

/// Where a link leads.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    /// `[[Note]]`, `[[Note#Heading]]`, `![[file]]`, `[text](Note.md)` in the note `from`
    Link { target: String, heading: Option<String>, from: PathBuf },
    /// `[text](url)` or a bare address
    Web { url: String },
}

impl Target {
    /// What following the link does, the same for a click, `gt` (EDIT-023) and Enter in the preview.
    pub fn follow(&self, new_tab: bool) -> EditorAction {
        match self {
            Target::Link { target, heading, .. } => EditorAction::OpenLink { target: target.clone(), heading: heading.clone(), new_tab },
            Target::Web { url } => EditorAction::OpenUrl(url.clone()),
        }
    }
}

/// The link at byte `byte` of the note `from`, if there is one. Embeds count (a picture has a preview).
pub fn target_at(text: &str, byte: usize, from: &Path) -> Option<Target> {
    if let Some(l) = markdown::find_links(text).into_iter().find(|l| l.range.contains(&byte)) {
        return Some(Target::Link { target: l.target, heading: l.heading, from: from.to_path_buf() });
    }
    lists::find_web_links(text).into_iter().find(|w| w.range.contains(&byte)).map(|w| Target::Web { url: w.url })
}

// ---------------------------------------------------------------------------------------------
// Web pages

/// What is read of a web page.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WebPage {
    pub title: String,
    pub description: String,
    /// the start of the readable text, one paragraph per line
    pub text: String,
}

/// `www.example.org` is a web address, but has no scheme to fetch it with.
pub fn fetchable(url: &str) -> String {
    if url.to_ascii_lowercase().starts_with("www.") {
        format!("https://{url}")
    } else {
        url.to_string()
    }
}

/// Download a page and read it (blocking: run it in a job). Plain https (http for this computer only,
/// `dav::check_url`), no login, 30 seconds, at most `MAX_PAGE_BYTES`.
pub fn fetch_page(url: &str) -> Result<WebPage, String> {
    let url = fetchable(url);
    if url.to_ascii_lowercase().starts_with("mailto:") {
        return Err("this is a mail address, not a web page".into());
    }
    let acct = dav::Account { url: String::new(), username: String::new(), password: crate::secret::Secret::default() };
    let r = dav::request_bytes("GET", &url, &["Accept: text/html,application/xhtml+xml;q=0.9,text/plain;q=0.5", "User-Agent: Shale link preview"], "", &acct, MAX_PAGE_BYTES)?;
    match r.status {
        200..=299 => {}
        404 | 410 => return Err(format!("the page was not found ({})", r.status)),
        401 | 403 => return Err(format!("the server does not show this page without a login ({})", r.status)),
        s => return Err(format!("the server answered with status {s}")),
    }
    if let Some(ct) = r.content_type.as_deref().filter(|c| !(c.starts_with("text/") || c.contains("html") || c.contains("xml"))) {
        return Err(format!("this is not a web page but a file of type {ct}"));
    }
    let html = String::from_utf8_lossy(&r.body);
    let page = parse_page(&html);
    if page == WebPage::default() {
        return Err("the page has nothing to show".into());
    }
    Ok(page)
}

/// Text between `<` `>` tags that is dropped together with what is inside them.
const SKIPPED: [&str; 9] = ["script", "style", "noscript", "svg", "template", "nav", "footer", "aside", "iframe"];

/// The title, the description and the start of the readable text of an HTML page.
pub fn parse_page(html: &str) -> WebPage {
    let lower = html.to_ascii_lowercase();
    let (mut title, mut og_title, mut description, mut og_description) = (String::new(), String::new(), String::new(), String::new());
    let (mut all, mut main) = (String::new(), String::new());
    let mut depth_main = 0usize;
    let mut i = 0;
    let bytes = html.as_bytes();
    while i < html.len() {
        if bytes[i] != b'<' {
            let next = html[i..].find('<').map_or(html.len(), |p| i + p);
            let chunk = &html[i..next];
            all.push_str(chunk);
            if depth_main > 0 {
                main.push_str(chunk);
            }
            i = next;
            continue;
        }
        if html[i..].starts_with("<!--") {
            i = html[i..].find("-->").map_or(html.len(), |p| i + p + 3);
            continue;
        }
        let Some(end) = tag_end(html, i) else { break };
        let inner = &html[i + 1..end];
        i = end + 1;
        let closing = inner.starts_with('/');
        let name: String = inner.trim_start_matches('/').chars().take_while(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
        if name.is_empty() {
            continue;
        }
        if !closing && SKIPPED.contains(&name.as_str()) {
            // up to the closing tag (a self-closing one like <svg/> has none to wait for)
            if !inner.ends_with('/') {
                i = lower[i..].find(&format!("</{name}")).and_then(|p| tag_end(html, i + p)).map_or(html.len(), |e| e + 1);
            }
            continue;
        }
        match (name.as_str(), closing) {
            ("title", false) => {
                let stop = lower[i..].find("</title").map_or(html.len(), |p| i + p);
                title = decode(&html[i..stop]);
                i = stop;
            }
            ("meta", false) => {
                let attrs = attributes(inner);
                let get = |k: &str| attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
                let key = get("property").or_else(|| get("name")).unwrap_or_default().to_ascii_lowercase();
                let content = decode(&get("content").unwrap_or_default());
                match key.as_str() {
                    "og:title" => og_title = content,
                    "description" => description = content,
                    "og:description" => og_description = content,
                    _ => {}
                }
            }
            ("main" | "article", false) => depth_main += 1,
            ("main" | "article", true) => depth_main = depth_main.saturating_sub(1),
            _ => {}
        }
        // a tag that ends a line of text
        if matches!(name.as_str(), "p" | "div" | "br" | "li" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "tr" | "section" | "article" | "main" | "blockquote" | "pre" | "ul" | "ol" | "table" | "header" | "dd" | "dt") {
            all.push('\n');
            if depth_main > 0 {
                main.push('\n');
            }
        }
    }
    let readable = |s: &str| paragraphs(&decode(s));
    let main = readable(&main);
    let text = if main.chars().count() >= 200 { main } else { readable(&all) };
    let first = |a: String, b: String| if a.trim().is_empty() { b } else { a };
    WebPage { title: clean(&first(title, og_title)), description: clean(&first(og_description, description)), text }
}

/// The index of the `>` that ends the tag starting at `start`, not counting one inside quotes.
fn tag_end(html: &str, start: usize) -> Option<usize> {
    let mut quote = None;
    for (i, c) in html[start + 1..].char_indices() {
        match (quote, c) {
            (None, '>') => return Some(start + 1 + i),
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), c) if c == q => quote = None,
            _ => {}
        }
    }
    None
}

/// The `name="value"` pairs of a tag (names lower case).
fn attributes(tag: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let rest = tag.trim_start_matches('/').trim_start_matches(|c: char| c.is_ascii_alphanumeric());
    let chars: Vec<char> = rest.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        while i < chars.len() && (chars[i].is_whitespace() || chars[i] == '/') {
            i += 1;
        }
        let start = i;
        while i < chars.len() && !chars[i].is_whitespace() && chars[i] != '=' && chars[i] != '/' {
            i += 1;
        }
        let name: String = chars[start..i].iter().collect::<String>().to_ascii_lowercase();
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        let mut value = String::new();
        if i < chars.len() && chars[i] == '=' {
            i += 1;
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            if i < chars.len() && (chars[i] == '"' || chars[i] == '\'') {
                let q = chars[i];
                i += 1;
                while i < chars.len() && chars[i] != q {
                    value.push(chars[i]);
                    i += 1;
                }
                i += 1;
            } else {
                while i < chars.len() && !chars[i].is_whitespace() {
                    value.push(chars[i]);
                    i += 1;
                }
            }
        }
        if !name.is_empty() {
            out.push((name, value));
        }
    }
    out
}

/// `&amp;`, `&#39;`, `&#x27;`, `&nbsp;` … as the characters they stand for.
fn decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(p) = rest.find('&') {
        out.push_str(&rest[..p]);
        rest = &rest[p..];
        let entity = rest[1..].find([';', ' ', '&', '<']).filter(|&e| rest.as_bytes()[1 + e] == b';' && e <= 10).map(|e| &rest[1..1 + e]);
        let decoded = entity.and_then(|e| match e {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            "hellip" => Some('…'),
            "mdash" => Some('—'),
            "ndash" => Some('–'),
            _ => e
                .strip_prefix('#')
                .and_then(|n| match n.strip_prefix(['x', 'X']) {
                    Some(h) => u32::from_str_radix(h, 16).ok(),
                    None => n.parse().ok(),
                })
                .and_then(char::from_u32),
        });
        match (decoded, entity) {
            (Some(c), Some(e)) => {
                out.push(c);
                rest = &rest[e.len() + 2..];
            }
            _ => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// One line of text: whitespace folded.
fn clean(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The start of the readable text: the lines that are more than a menu entry, one paragraph per line.
fn paragraphs(text: &str) -> String {
    let lines: Vec<String> = text.lines().map(clean).filter(|l| !l.is_empty()).collect();
    let long: Vec<&String> = lines.iter().filter(|l| l.chars().count() >= 30).collect();
    let chosen: Vec<&String> = if long.is_empty() { lines.iter().collect() } else { long };
    let mut out = String::new();
    for p in chosen.into_iter().take(MAX_PARAGRAPHS) {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(p);
        if out.chars().count() > MAX_TEXT_CHARS {
            out = out.chars().take(MAX_TEXT_CHARS).collect::<String>().trim_end().to_string() + "…";
            break;
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// What the preview shows

/// What the preview of a link holds, found when it opens.
pub enum Content {
    /// a note, from the heading of the link on
    Note {
        rel: PathBuf,
        text: String,
        /// the numbers of its headings by line of `text` (LINK-014); none when numbering is off
        numbers: std::collections::HashMap<usize, String>,
    },
    /// no such note (Enter creates it)
    Missing(String),
    Image {
        abs: PathBuf,
        rel: PathBuf,
    },
    /// a drawing, a PDF, any other file: its path and what it is
    File {
        rel: PathBuf,
        kind: &'static str,
    },
    Web {
        url: String,
    },
    /// a link that has nothing to preview, in words
    Plain(String),
}

/// The content for a link of the note `from`; `note_text` gives the text of a note (the open buffer
/// when the note is open).
pub fn content_for(target: &Target, vault: &Vault, note_text: &dyn Fn(&Path) -> Option<String>, numbered: bool) -> Content {
    let (link, heading, from) = match target {
        Target::Web { url } if url.to_ascii_lowercase().starts_with("mailto:") => return Content::Plain(format!("A mail to {}. Enter opens your mail program.", url[7..].split('?').next().unwrap_or(""))),
        Target::Web { url } => return Content::Web { url: fetchable(url) },
        Target::Link { target, heading, from } => (target, heading, from),
    };
    let Some(rel) = vault.resolve(link, Some(from)) else { return Content::Missing(link.clone()) };
    match vault.file(&rel).map(|f| f.kind) {
        Some(FileKind::Note) => {
            let text = note_text(&rel).unwrap_or_default();
            let (start, shown) = from_heading(&text, heading.as_deref());
            // the numbers are those of the whole note, also when the preview starts at a heading
            let numbers = reader::Extras::numbers_of(&text, numbered).into_iter().filter_map(|(l, n)| l.checked_sub(start).map(|l| (l, n))).collect();
            Content::Note { text: shown, numbers, rel }
        }
        Some(FileKind::Drawing) => Content::File { rel, kind: "Drawing" },
        _ if crate::media::is_image(&rel) => Content::Image { abs: vault.abs(&rel), rel },
        _ => {
            let kind = if rel.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf")) { "PDF" } else { "File" };
            Content::File { rel, kind }
        }
    }
}

/// The line of `heading` and the text from it on (the whole text from line 0 when there is no
/// heading or it is not found), cut at `MAX_NOTE_CHARS`.
fn from_heading(text: &str, heading: Option<&str>) -> (usize, String) {
    let start = heading.and_then(|h| markdown::find_heading(text, h)).unwrap_or(0);
    let mut out = String::new();
    for line in text.lines().skip(start) {
        if out.chars().count() + line.chars().count() > MAX_NOTE_CHARS {
            out.push('…');
            break;
        }
        out.push_str(line);
        out.push('\n');
    }
    (start, out)
}

/// What the keys of the preview ask the app to do (EDIT-035). One place carries it out:
/// `ShaleApp::link_preview_outcome`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Stay,
    /// close it, the keyboard is the editor's again
    Close,
    /// follow the link as `gt` does, and close it
    Open,
    /// Ctrl+Enter: follow the link in a new pane right of this one, and close it (EDIT-036)
    OpenInSplit,
}

/// The open preview.
pub struct Open {
    pub target: Target,
    pub content: Content,
    /// the keyboard is in the preview
    pub focused: bool,
    /// what it was opened for: this tab, this cursor, this text
    pub tab: u64,
    pub cursor: usize,
    pub text_hash: u64,
    /// the row of the cursor on the screen
    pub anchor: Rect,
    /// the same id every frame, so that the frame and its scroll area keep their size and place
    pub id: egui::Id,
    /// where the frame was drawn last (a click outside it closes the preview)
    pub rect: Option<Rect>,
    pub scroll: f32,
    /// the size of the frame, from the settings when it opened, changed by dragging its edges (EDIT-037)
    pub size: Vec2,
    /// what the frame's margins and stroke take of its size
    pub pad: Vec2,
    /// how far dragging the left edge moved the frame sideways
    pub shift_x: f32,
    /// the frame is being dragged by an edge: it is as big as `size`, not as the content
    pub resizing: bool,
    /// the height of the content drawn last frame: the frame is as tall (at most `size`)
    height: f32,
    max_scroll: f32,
    page: f32,
}

impl Open {
    pub fn new(target: Target, content: Content, (tab, cursor, text_hash): (u64, usize, u64), anchor: Rect, size: Vec2) -> Open {
        Open {
            target,
            content,
            focused: false,
            tab,
            cursor,
            text_hash,
            anchor,
            rect: None,
            id: egui::Id::unique("link-preview"),
            scroll: 0.0,
            size,
            pad: Vec2::splat(14.0),
            shift_x: 0.0,
            resizing: false,
            height: 24.0,
            max_scroll: f32::MAX,
            page: size.y * 0.9,
        }
    }

    /// Drag the handle `grip` by `delta` (EDIT-037): the size follows, within `clamp_size` of `screen`; the left
    /// edge moves the frame sideways with it. The edge at the cursor row stays where it is, the frame hangs there.
    pub fn resize(&mut self, grip: Grip, delta: Vec2, screen: Vec2) {
        let want = self.size + Vec2::new(delta.x * f32::from(grip.x), delta.y * f32::from(grip.y));
        let new = clamp_size(want, screen);
        if grip.x < 0 {
            self.shift_x -= new.x - self.size.x;
        }
        self.size = new;
    }

    fn scroll_by(&mut self, d: f32) {
        self.scroll = (self.scroll + d).clamp(0.0, self.max_scroll);
    }

    /// Take the keys of this frame. Not focused: only Esc, which closes. Focused: every key and typed
    /// character is taken (Ctrl / Alt chords stay for the shortcuts of the window).
    pub fn keys(&mut self, input: &mut InputState) -> Outcome {
        let mut out = Outcome::Stay;
        let focused = self.focused;
        let mut moves = Vec::new();
        input.events.retain(|e| match e {
            Event::Key { key, pressed, modifiers, .. } if focused || *key == Key::Escape => {
                let ctrl = modifiers.command || modifiers.ctrl;
                if ctrl && !modifiers.alt && matches!(key, Key::D | Key::U) {
                    if *pressed {
                        moves.push(if *key == Key::D { self.page } else { -self.page });
                    }
                    return false;
                }
                if ctrl && !modifiers.alt && !modifiers.shift && *key == Key::Enter {
                    if *pressed && out == Outcome::Stay {
                        out = Outcome::OpenInSplit;
                    }
                    return false;
                }
                if ctrl || modifiers.alt {
                    return true;
                }
                if *pressed && out == Outcome::Stay {
                    match key {
                        Key::Escape => out = Outcome::Close,
                        Key::Enter => out = Outcome::Open,
                        Key::J | Key::ArrowDown => moves.push(LINE),
                        Key::K | Key::ArrowUp => moves.push(-LINE),
                        Key::PageDown => moves.push(self.page),
                        Key::PageUp => moves.push(-self.page),
                        _ => {}
                    }
                }
                false
            }
            Event::Text(_) | Event::Ime(_) => !focused,
            _ => true,
        });
        for d in moves {
            self.scroll_by(d);
        }
        out
    }
}

/// What a download job delivers: the page or why not, or the message of a crashed job.
type JobResult = Result<Result<WebPage, String>, String>;

/// The preview and what it remembers: the open one, the web pages read so far this session and the
/// download in progress.
#[derive(Default)]
pub struct LinkPreview {
    pub open: Option<Open>,
    /// the pages read so far this session, by address (EDIT-034)
    pub pages: HashMap<String, Result<WebPage, String>>,
    loading: Option<(String, Receiver<JobResult>)>,
}

impl LinkPreview {
    /// Start reading `url` in the background unless it is known or already being read. `wake` repaints the window.
    pub fn load(&mut self, url: &str, wake: impl FnOnce() + Send + 'static) {
        if self.pages.contains_key(url) || self.loading.as_ref().is_some_and(|(u, _)| u == url) {
            return;
        }
        let u = url.to_string();
        self.loading = Some((url.to_string(), crate::jobs::spawn(move || fetch_page(&u), wake)));
    }

    /// Take in a finished download; true while one is still running.
    pub fn poll(&mut self) -> bool {
        let Some((url, rx)) = &self.loading else { return false };
        match rx.try_recv() {
            Ok(r) => {
                let url = url.clone();
                self.pages.insert(url, r.and_then(|page| page));
                self.loading = None;
                false
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => true,
            Err(_) => {
                let url = url.clone();
                self.pages.insert(url, Err("the download was lost".into()));
                self.loading = None;
                false
            }
        }
    }

    /// Draw the content of the open preview; a click on a link of a note is returned.
    pub fn show(&mut self, ui: &mut Ui, vault: &Vault, caches: &mut reader::Caches, base: f32, dark: bool) -> Option<(reader::ReaderAction, PathBuf)> {
        let open = self.open.as_mut()?;
        let mut action = None;
        let room = open.size - open.pad;
        // the content's height, but as tall as the frame while it is dragged
        let shown = if open.resizing { room.y } else { open.height.min(room.y) };
        let area = egui::ScrollArea::vertical().min_scrolled_height(shown).max_height(shown).auto_shrink([false, false]).vertical_scroll_offset(open.scroll).show(ui, |ui| {
            ui.set_width(ui.available_width());
            match &open.content {
                Content::Note { rel, text, numbers } => {
                    if text.trim().is_empty() {
                        ui.label(egui::RichText::new("The note is empty").color(theme::MUTED));
                    } else if let Some(a) = reader::show(ui, text, vault, rel, caches, base, dark, &reader::Extras { numbers: numbers.clone(), scroll_to: None }) {
                        action = Some((a, rel.clone()));
                    }
                }
                Content::Missing(name) => {
                    ui.label(egui::RichText::new("Note does not exist yet").strong().color(theme::LINK_MISSING));
                    ui.label(egui::RichText::new(format!("“{name}” is created when you press Enter")).color(theme::MUTED));
                }
                Content::Image { abs, rel } => match crate::media::image_widget(ui.ctx(), abs, room.x, room.y - 16.0) {
                    Some(img) => {
                        ui.add(img);
                    }
                    None => {
                        ui.label(egui::RichText::new(rel.to_string_lossy()).color(theme::TEXT));
                    }
                },
                Content::File { rel, kind } => {
                    ui.label(egui::RichText::new(rel.to_string_lossy()).strong().color(theme::TEXT));
                    ui.label(egui::RichText::new(format!("{kind}. Enter opens it")).color(theme::MUTED));
                }
                Content::Plain(m) => {
                    ui.label(egui::RichText::new(m.as_str()).color(theme::TEXT));
                }
                Content::Web { url } => match self.pages.get(url) {
                    None => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(egui::RichText::new(format!("Loading {} …", dav::host_of(url))).color(theme::MUTED));
                        });
                    }
                    Some(Err(why)) => {
                        ui.label(egui::RichText::new("The page could not be loaded").strong().color(theme::LINK_MISSING));
                        ui.label(egui::RichText::new(why.as_str()).color(theme::TEXT));
                        ui.label(egui::RichText::new(url.as_str()).small().color(theme::MUTED));
                    }
                    Some(Ok(p)) => {
                        if !p.title.is_empty() {
                            ui.label(egui::RichText::new(p.title.as_str()).strong().size(base * 1.15).color(theme::HEADING));
                        }
                        ui.label(egui::RichText::new(url.as_str()).small().color(theme::LINK));
                        if !p.description.is_empty() {
                            ui.add_space(4.0);
                            ui.label(egui::RichText::new(p.description.as_str()).italics().color(theme::TEXT));
                        }
                        for para in p.text.lines() {
                            ui.add_space(4.0);
                            ui.label(egui::RichText::new(para).color(theme::TEXT));
                        }
                    }
                },
            }
        });
        // the wheel and the scroll bar move it too: what the area has now is the offset
        open.scroll = area.state.clamped_offset().y;
        open.max_scroll = (area.content_size.y - area.inner_rect.height()).max(0.0);
        open.page = (area.inner_rect.height() * 0.9).max(LINE);
        if (area.content_size.y - open.height).abs() > 0.5 {
            // the content got another size (a page arrived): the next frame has it
            open.height = area.content_size.y;
            ui.ctx().request_repaint();
        }
        action
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec: EDIT-037
    #[test]
    fn edit_037_the_size_stays_between_the_least_and_the_window() {
        let screen = Vec2::new(1500.0, 900.0);
        assert_eq!(clamp_size(Vec2::new(640.0, 480.0), screen), Vec2::new(640.0, 480.0), "a size that fits stays");
        assert_eq!(clamp_size(Vec2::new(10.0, 20.0), screen), Vec2::new(MIN_WIDTH, MIN_HEIGHT), "not under the least");
        assert_eq!(clamp_size(Vec2::new(5000.0, 5000.0), screen), Vec2::new(1484.0, 884.0), "not over the window, with a margin");
        assert_eq!(clamp_size(Vec2::new(640.0, 480.0), Vec2::new(200.0, 100.0)), Vec2::new(184.0, 84.0), "a window smaller than the least wins");
        assert_eq!(clamp_size(Vec2::new(640.0, 480.0), Vec2::new(5.0, 5.0)), Vec2::new(1.0, 1.0), "and never below one point");
    }

    // spec: EDIT-037
    #[test]
    fn edit_037_dragging_a_handle_changes_the_size_of_its_edges() {
        let screen = Vec2::new(1500.0, 900.0);
        let target = Target::Web { url: "https://example.org/".into() };
        let by = |g: Grip, d: Vec2| {
            let mut o = Open::new(target.clone(), Content::Plain(String::new()), (1, 0, 0), Rect::ZERO, Vec2::new(640.0, 480.0));
            o.resize(g, d, screen);
            (o.size, o.shift_x)
        };
        let d = Vec2::new(30.0, 20.0);
        assert_eq!(by(Grip { x: 1, y: 1 }, d), (Vec2::new(670.0, 500.0), 0.0), "the corner changes both");
        assert_eq!(by(Grip { x: 1, y: 0 }, d), (Vec2::new(670.0, 480.0), 0.0), "the right edge only the width");
        assert_eq!(by(Grip { x: 0, y: 1 }, d), (Vec2::new(640.0, 500.0), 0.0), "the bottom edge only the height");
        assert_eq!(by(Grip { x: -1, y: 0 }, -d), (Vec2::new(670.0, 480.0), -30.0), "the left edge dragged left: wider, and the frame moves with it");
        assert_eq!(by(Grip { x: 0, y: -1 }, -d), (Vec2::new(640.0, 500.0), 0.0), "the top edge dragged up: taller");
        assert_eq!(by(Grip { x: -1, y: -1 }, Vec2::new(1000.0, 1000.0)).0, Vec2::new(MIN_WIDTH, MIN_HEIGHT), "not under the least");
        assert_eq!(by(Grip { x: -1, y: 0 }, Vec2::new(1000.0, 0.0)).1, 640.0 - MIN_WIDTH, "the frame moves only by what the width gave");
        // the handles: strips along the edges, squares at the corners, all around the frame's edge
        let r = Rect::from_min_size(Pos2::new(100.0, 100.0), Vec2::new(300.0, 200.0));
        let rects: Vec<Rect> = Grip::ALL.iter().map(|g| g.rect(r)).collect();
        assert!(rects.iter().all(|h| h.width() > 0.0 && h.height() > 0.0 && r.expand(GRAB).contains_rect(*h)));
        let overlap = |a: &Rect, b: &Rect| a.intersect(*b).width() > 0.0 && a.intersect(*b).height() > 0.0;
        assert!(rects.iter().enumerate().all(|(i, a)| rects.iter().skip(i + 1).all(|b| !overlap(a, b))), "no two handles overlap");
        assert_eq!(Grip { x: 1, y: 1 }.rect(r), Rect::from_center_size(r.max, Vec2::splat(2.0 * GRAB)));
        use egui::CursorIcon::{ResizeHorizontal as H, ResizeNeSw as NeSw, ResizeNwSe as NwSe, ResizeVertical as V};
        assert_eq!(Grip::ALL.map(Grip::cursor), [H, H, V, V, NwSe, NeSw, NeSw, NwSe]);
    }

    const PAGE: &str = r#"<!DOCTYPE html><html><head><meta charset="utf-8">
<title>The &amp; Title</title>
<meta name="description" content="Plain description">
<meta property="og:description" content='An &quot;open&quot; description'>
<style>body { color: red }</style><script>var x = "<p>not text</p>";</script></head>
<body><nav>Home | About | Contact us now please</nav>
<main><h1>Heading of the page</h1>
<!-- a comment <p>hidden</p> -->
<p>The first paragraph is long enough to count as <b>text</b> and has an entity &#8212; and &#x41;.</p>
<p>Short</p>
<p>The second paragraph is long enough to count as text of the page too.</p></main>
<footer>Copyright someone, all rights reserved here</footer></body></html>"#;

    // spec: EDIT-034
    #[test]
    fn parse_page_reads_title_description_and_text() {
        let p = parse_page(PAGE);
        assert_eq!(p.title, "The & Title");
        assert_eq!(p.description, "An \"open\" description", "og:description wins over description");
        let lines: Vec<&str> = p.text.lines().collect();
        assert_eq!(lines, ["The first paragraph is long enough to count as text and has an entity — and A.", "The second paragraph is long enough to count as text of the page too."], "{lines:?}");
        assert!(!p.text.contains("hidden") && !p.text.contains("var x") && !p.text.contains("Home") && !p.text.contains("Copyright"));
    }

    // spec: EDIT-034
    #[test]
    fn parse_page_falls_back_and_stays_bounded() {
        // no description, no main: the og:title and the body text
        let p = parse_page(r#"<meta name="description" content="D"><meta property="og:title" content="Og"><body><div>One line of the body that is long enough to be text.</div></body>"#);
        assert_eq!((p.title.as_str(), p.description.as_str()), ("Og", "D"));
        assert_eq!(p.text, "One line of the body that is long enough to be text.");
        // a broken tag does not loop or panic; plain text is text
        assert_eq!(parse_page("just words <b").text, "just words");
        assert_eq!(parse_page(""), WebPage::default());
        // many long paragraphs: at most 8, and not more than the limit of characters
        let long = format!("<p>{}</p>", "word ".repeat(200));
        let p = parse_page(&long.repeat(20));
        assert!(p.text.lines().count() <= MAX_PARAGRAPHS && p.text.chars().count() <= MAX_TEXT_CHARS + 1, "{}", p.text.chars().count());
        assert!(p.text.ends_with('…'));
        // entities that are none stay
        assert_eq!(decode("a &b; &amp c &#xZZ; &lt;"), "a &b; &amp c &#xZZ; <");
    }

    // spec: EDIT-033
    #[test]
    fn the_link_under_the_cursor_is_the_target() {
        let text = "see [[Ideas#Top]] and [site](https://example.org/x) or https://bare.example, ![[pic.png]] and [m](Notes/Plan.md) end";
        let at = |w: &str| text.find(w).unwrap() + 1;
        let from = Path::new("A.md");
        let link = |t: &str, h: Option<&str>| Some(Target::Link { target: t.into(), heading: h.map(String::from), from: from.into() });
        assert_eq!(target_at(text, at("Ideas"), from), link("Ideas", Some("Top")));
        assert_eq!(target_at(text, at("[[Ideas"), from), link("Ideas", Some("Top")), "on the first bracket");
        assert_eq!(target_at(text, at("site"), from), Some(Target::Web { url: "https://example.org/x".into() }));
        assert_eq!(target_at(text, at("bare"), from), Some(Target::Web { url: "https://bare.example".into() }));
        assert_eq!(target_at(text, at("pic.png"), from), link("pic.png", None), "an embed");
        assert_eq!(target_at(text, at("Plan"), from), link("Notes/Plan", None));
        assert_eq!(target_at(text, at("see"), from), None, "off a link");
        assert_eq!(target_at(text, text.len(), from), None);
        // what following does is what gt does
        assert!(matches!(link("Ideas", Some("Top")).unwrap().follow(false), EditorAction::OpenLink { new_tab: false, .. }));
        assert!(matches!(Target::Web { url: "u".into() }.follow(false), EditorAction::OpenUrl(u) if u == "u"));
    }

    // spec: EDIT-034
    #[test]
    fn the_note_preview_starts_at_the_heading() {
        let text = "intro\n# One\nfirst\n## Two\nsecond\n";
        assert_eq!(from_heading(text, None).1, text);
        assert_eq!(from_heading(text, Some("two")), (3, "## Two\nsecond\n".to_string()));
        assert_eq!(from_heading(text, Some("nope")).1, text, "an unknown heading shows the note from the top");
        let big = "line of text\n".repeat(5000);
        assert!(from_heading(&big, None).1.chars().count() <= MAX_NOTE_CHARS + 1);
        assert_eq!(fetchable("www.example.org"), "https://www.example.org");
        assert_eq!(fetchable("http://a.b"), "http://a.b");
    }
}
