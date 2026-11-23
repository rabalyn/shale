//! The state of the compose box (a new post, a reply or a quote, SOC-051) and what happens around
//! it: counting the characters the way each network does (SOC-052), opening it with the
//! mention of the replied-to author, sending it in a background job, and what the feed shows
//! when the post is out. The box itself is drawn by `compose_ui.rs`.

use super::actions::*;
use super::state::{run, Msg, SocialState};
use super::*;
use eframe::egui::Context;
use std::path::{Path, PathBuf};

/// Where the box is in its steps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Edit,
    /// "Post this?" waits for Post or Cancel (SOC-055)
    Confirm,
    /// the job runs: the box cannot be changed or sent again
    Sending,
}

/// A post being written.
#[derive(Clone, Debug)]
pub struct Compose {
    /// the Feeds tab the box is shown in
    pub tab: u64,
    pub account: String,
    /// the feed of that tab (the post is indexed with it)
    pub feed: String,
    pub reply: Option<Box<Post>>,
    pub quote: Option<Box<Post>>,
    pub text: String,
    /// Mastodon: the content warning field is open
    pub cw_on: bool,
    pub cw: String,
    pub visibility: Visibility,
    pub pictures: Vec<DraftPicture>,
    /// the path of a picture being typed
    pub path: String,
    pub stage: Stage,
    pub error: Option<String>,
    /// the text field takes the keyboard when the box opens
    pub focus: bool,
}

/// Characters of a text as Mastodon counts them: an address counts 23, a mention `@user@host` only `@user`.
pub fn mastodon_length(text: &str) -> usize {
    text.split_inclusive(char::is_whitespace)
        .map(|word| {
            let bare = word.trim_end_matches(char::is_whitespace);
            let tail = word.chars().count() - bare.chars().count();
            let lead = bare.chars().take_while(|c| matches!(c, '(' | '[' | '{' | '"' | '\'')).count();
            let tok: String = bare.chars().skip(lead).collect();
            let url = tok.trim_end_matches(['.', ',', ';', ':', '!', '?', ')', ']', '}', '"', '\'']);
            let n = if url.starts_with("https://") || url.starts_with("http://") {
                lead + 23 + (tok.chars().count() - url.chars().count())
            } else if tok.starts_with('@') && tok[1..].contains('@') {
                // @user@host counts as @user
                lead + 1 + tok[1..].split('@').next().unwrap_or("").chars().count()
            } else {
                bare.chars().count()
            };
            n + tail
        })
        .sum()
}

impl Compose {
    pub fn new(tab: u64, account: &str, feed: &str) -> Compose {
        Compose {
            tab,
            account: account.to_string(),
            feed: feed.to_string(),
            reply: None,
            quote: None,
            text: String::new(),
            cw_on: false,
            cw: String::new(),
            visibility: Visibility::Public,
            pictures: vec![],
            path: String::new(),
            stage: Stage::Edit,
            error: None,
            focus: true,
        }
    }

    /// Whether anything is written (closing it would lose something).
    pub fn has_content(&self) -> bool {
        !self.text.trim().is_empty() || !self.pictures.is_empty() || !self.cw.trim().is_empty()
    }

    /// The text that is sent: on Mastodon a quote the server cannot make carries the post's address (SOC-054).
    pub fn final_text(&self, network: Network, info: &InstanceInfo) -> String {
        let text = self.text.trim_end().to_string();
        match (&self.quote, network, info.quotes) {
            (Some(q), Network::Mastodon, false) if !q.url.is_empty() => {
                if text.is_empty() {
                    q.url.clone()
                } else {
                    format!("{text}\n\n{}", q.url)
                }
            }
            _ => text,
        }
    }

    /// Whether the quote is made with the server's own quote (Bluesky always; Mastodon when the server can).
    pub fn real_quote(&self, network: Network, info: &InstanceInfo) -> bool {
        self.quote.is_some() && (network == Network::Bluesky || info.quotes)
    }

    /// How long the post is in the unit of its network (SOC-052); a Mastodon content warning counts too.
    pub fn length(&self, network: Network, info: &InstanceInfo) -> usize {
        let text = self.final_text(network, info);
        match network {
            Network::Mastodon => mastodon_length(&text) + if self.cw_on { self.cw.trim().chars().count() } else { 0 },
            Network::Bluesky => bluesky::grapheme_count(&text),
        }
    }

    /// Why the post cannot be sent now; None when it can.
    pub fn problem(&self, network: Network, info: &InstanceInfo) -> Option<&'static str> {
        let text_empty = self.text.trim().is_empty();
        if text_empty && self.pictures.is_empty() {
            return Some("Write something or add a picture");
        }
        if self.length(network, info) > info.max_chars {
            return Some("The post is too long");
        }
        None
    }

    /// Add the picture whose path is typed: a path of the vault (relative to its root) or any file (SOC-053).
    pub fn add_picture(&mut self, vault: Option<&Path>, info: &InstanceInfo) -> Result<(), String> {
        let typed = self.path.trim().trim_matches(['"', '\'']).to_string();
        if typed.is_empty() {
            return Err("Type the path of a picture first.".into());
        }
        if self.pictures.len() >= info.max_pictures {
            return Err(format!("A post can have at most {} pictures.", info.max_pictures));
        }
        let mut path = PathBuf::from(&typed);
        if path.is_relative() {
            path = vault.ok_or("Open a vault first, or type the full path of the picture.")?.join(&path);
        }
        let mut head = [0u8; 32];
        let n = crate::fsutil::open_read(&path).and_then(|mut f| std::io::Read::read(&mut f, &mut head)).map_err(|e| format!("{typed} cannot be read ({e})"))?;
        if !matches!(image::guess_format(&head[..n]), Ok(image::ImageFormat::Png | image::ImageFormat::Jpeg | image::ImageFormat::Gif | image::ImageFormat::WebP)) {
            return Err(format!("{typed} is not a JPEG, PNG, GIF or WebP picture."));
        }
        if self.pictures.iter().any(|p| p.path == path) {
            return Err("This picture is in the post already.".into());
        }
        self.pictures.push(DraftPicture { path, alt: String::new() });
        self.path.clear();
        Ok(())
    }

    /// The post as it is sent.
    pub fn draft(&self, network: Network, info: &InstanceInfo) -> Draft {
        Draft {
            text: self.final_text(network, info),
            cw: if network == Network::Mastodon && self.cw_on { self.cw.trim().to_string() } else { String::new() },
            visibility: self.visibility,
            reply_to: self.reply.as_deref().cloned(),
            quote: if self.real_quote(network, info) { self.quote.as_deref().cloned() } else { None },
            pictures: self.pictures.clone(),
            info: info.clone(),
        }
    }
}

impl SocialState {
    /// What the account's server allows (the defaults until its answer is in).
    pub fn info_for(&self, account: &Account) -> InstanceInfo {
        self.writes.info.get(&account.id).cloned().unwrap_or_else(|| InstanceInfo::defaults(account.network))
    }

    /// Ask a Mastodon server what it allows (once per account and run).
    fn ensure_info(&mut self, ctx: &Context, e: &SocialAccountEntry) {
        let id = e.account.id.clone();
        if e.account.network != Network::Mastodon || self.writes.info.contains_key(&id) || !self.writes.asking.insert(id.clone()) {
            return;
        }
        let (client, src) = (self.client_source(e), self.source.clone());
        let id2 = id.clone();
        run(
            ctx,
            &self.tx,
            move || {
                let (client, result) = match src {
                    Some(s) => (None, s.instance()),
                    None => {
                        let mut c = client();
                        let r = c.instance_info();
                        (Some(c), r)
                    }
                };
                Msg::Instance { account: id, client, result }
            },
            move |m| Msg::Instance { account: id2, client: None, result: Err(m) },
        );
    }

    /// Open the compose box in a tab: a new post, a reply to `reply`, or a quote of `quote`. A
    /// Mastodon account that cannot write is sent to its sign-in first (SOC-056). A box with
    /// something written is kept.
    pub fn open_compose(&mut self, ctx: &Context, e: &SocialAccountEntry, tab: u64, feed: &str, reply: Option<&Post>, quote: Option<&Post>) {
        if e.account.may_write() == Some(false) {
            self.need_write(&e.account.id);
            return;
        }
        if self.compose.as_ref().is_some_and(|c| c.stage == Stage::Sending || c.has_content()) {
            self.messages.push("Finish or cancel the post you are writing first".into());
            return;
        }
        self.ensure_info(ctx, e);
        let mut c = Compose::new(tab, &e.account.id, feed);
        c.visibility = self.writes.last_visibility;
        if let Some(p) = reply {
            if e.account.network == Network::Mastodon {
                if !p.author.handle.eq_ignore_ascii_case(&e.account.handle) {
                    c.text = format!("@{} ", p.author.handle);
                }
                // a reply is not more public than what it answers
                if let Some(v) = p.visibility.as_deref().and_then(Visibility::from_key).filter(|v| *v != Visibility::Public) {
                    c.visibility = v;
                }
            }
            c.reply = Some(Box::new(Post { reposted_by: None, ..p.clone() }));
        }
        if let Some(q) = quote {
            c.quote = Some(Box::new(Post { reposted_by: None, ..q.clone() }));
        }
        self.compose = Some(c);
    }

    /// Send what the box holds, in a background job (SOC-051). The box waits, disabled, until
    /// the job ends; a failure puts it back with the message and the text kept.
    pub fn send_compose(&mut self, ctx: &Context, e: &SocialAccountEntry) {
        let info = self.info_for(&e.account);
        let Some(c) = self.compose.as_mut().filter(|c| c.account == e.account.id && c.stage != Stage::Sending) else { return };
        if c.problem(e.account.network, &info).is_some() {
            return;
        }
        c.stage = Stage::Sending;
        c.error = None;
        let (draft, feed) = (c.draft(e.account.network, &info), c.feed.clone());
        let (client, src) = (self.client_source(e), self.source.clone());
        let account = e.account.id.clone();
        let (account2, feed2) = (account.clone(), feed.clone());
        run(
            ctx,
            &self.tx,
            move || {
                let (client, result) = match src {
                    Some(s) => (None, s.send(&draft)),
                    None => {
                        let mut c = client();
                        let r = c.send(&draft);
                        (Some(c), r)
                    }
                };
                Msg::Sent { account, feed, client, result: result.map(Box::new) }
            },
            move |m| Msg::Sent { account: account2, feed: feed2, client: None, result: Err(ActionError::Failed(m)) },
        );
    }

    /// The send ended. A new post waits in the account's own feed (it is not put into the list, so the
    /// view stays still, SOC-027); a reply goes into the conversation that is open (SOC-057).
    pub(super) fn sent(&mut self, account: &str, feed: &str, result: Result<Box<Post>, ActionError>) {
        match result {
            Ok(post) => {
                let post = *post;
                if let Some(c) = self.compose.take_if(|c| c.account == account) {
                    if post.network == Network::Mastodon {
                        self.writes.last_visibility = c.visibility;
                    }
                }
                self.index.add(account, feed, std::slice::from_ref(&post));
                if let Some(parent) = post.reply_to.clone() {
                    self.update_post(account, &parent, |p| p.reply_count = p.reply_count.saturating_add(1));
                    self.threads.add_reply(account, &post);
                }
                for ((a, f), t) in self.timelines.iter_mut() {
                    if a == account && matches!(f.as_str(), "home" | "timeline") && t.loaded && !t.waiting.iter().chain(&t.posts).any(|p| p.id == post.id) {
                        t.waiting.insert(0, post.clone());
                    }
                }
                self.messages.push("Posted".into());
            }
            Err(e) => {
                if let Some(c) = self.compose.as_mut().filter(|c| c.account == account) {
                    c.stage = Stage::Edit;
                    c.error = Some(e.to_string());
                }
                if e == ActionError::NeedsWrite {
                    self.writes.needs_write.push(account.to_string());
                }
            }
        }
    }
}
