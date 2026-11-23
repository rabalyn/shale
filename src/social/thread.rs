//! What the thread view of a Feeds tab holds: the conversation of a post (what it replies to, and
//! the replies below it), fetched in a background job per account and post (SOC-045). The view
//! itself is in `thread_ui.rs`, the job is started by `SocialState::fetch_thread`.

use super::{Post, ThreadView};
use std::collections::HashMap;
use std::sync::Arc;

pub enum ThreadStatus {
    /// opened, the job is not started yet
    Wanted,
    Loading,
    Ready(Arc<ThreadView>),
    Failed(String),
}

pub struct ThreadEntry {
    /// the post that was opened, as the feed showed it (drawn while the rest comes)
    pub post: Post,
    pub status: ThreadStatus,
}

/// The conversations opened in this run, by (account, post id). A step back and forth in a tab's
/// history shows the same data; opening a conversation from a post asks the server again.
#[derive(Default)]
pub struct Threads {
    pub map: HashMap<(String, String), ThreadEntry>,
    /// how high each post was drawn at a width (as `SocialState::card_heights`; separate because
    /// the opened post is drawn with a frame)
    pub heights: HashMap<(String, u32), (f32, bool)>,
    /// where each tab's conversation is held still while posts above the opened post change in
    /// height (SOC-046); a new conversation starts with the opened post at the top
    pub anchors: HashMap<u64, super::feed_ui::Anchor>,
}

impl Threads {
    /// A conversation was opened from `post`: it is fetched again (one that is on its way stays).
    pub fn open(&mut self, account: &str, post: &Post) {
        let key = (account.to_string(), post.id.clone());
        if matches!(self.map.get(&key), Some(ThreadEntry { status: ThreadStatus::Loading | ThreadStatus::Wanted, .. })) {
            return;
        }
        // a conversation opened again starts with the opened post at the top
        self.anchors.retain(|_, a| a.scope != key);
        self.map.insert(key, ThreadEntry { post: Post { reposted_by: None, ..post.clone() }, status: ThreadStatus::Wanted });
    }

    /// Ask again after an error.
    pub fn retry(&mut self, account: &str, id: &str) {
        if let Some(t) = self.map.get_mut(&(account.to_string(), id.to_string())).filter(|t| matches!(t.status, ThreadStatus::Failed(_))) {
            t.status = ThreadStatus::Wanted;
        }
    }

    /// The job ended. The posts that came, to be indexed (none after an error).
    pub fn arrived(&mut self, account: &str, id: &str, result: Result<Box<ThreadView>, String>) -> Vec<Post> {
        let Some(t) = self.map.get_mut(&(account.to_string(), id.to_string())) else { return vec![] };
        match result {
            Ok(v) => {
                let all = v.ancestors.iter().cloned().chain(std::iter::once(v.post.clone())).chain(v.replies.iter().map(|r| r.post.clone())).collect();
                t.status = ThreadStatus::Ready(Arc::new(*v));
                all
            }
            Err(e) => {
                t.status = ThreadStatus::Failed(e);
                vec![]
            }
        }
    }

    /// Forget the conversations of an account (removed).
    pub fn forget(&mut self, account: &str) {
        self.map.retain(|k, _| k.0 != account);
    }
}
